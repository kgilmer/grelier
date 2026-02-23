// Main application loop update/subscription logic shared by runtime and benchmarks.
use crate::apps;
use crate::bar::{
    AppIconCache, BarState, GaugeDialog, GaugeDialogWindow, Message, Orientation, close_window_task,
};
use crate::monitor;
use crate::panels;
use crate::panels::gauges::gauge::{GaugeClick, GaugeInput, GaugeModel};
use crate::panels::gauges::gauge_work_manager;
use crate::settings;
use crate::sway_workspace;
use elbey_cache::Cache;
use iced::{Subscription, Task, event, mouse, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use log::{error, info, warn};
use std::time::{Duration, Instant};

const DEFAULT_ORIENTATION: &str = "left";
const DIALOG_UNFOCUS_SUPPRESSION_WINDOW: Duration = Duration::from_millis(250);
const OUTPUT_REOPEN_SUPPRESSION_WINDOW: Duration = Duration::from_millis(750);

fn set_input_region_task(window: window::Id, size: iced::Size) -> Task<Message> {
    if size.width <= 0.0 || size.height <= 0.0 {
        return Task::none();
    }
    let width = size.width.round().clamp(1.0, i32::MAX as f32) as i32;
    let height = size.height.round().clamp(1.0, i32::MAX as f32) as i32;
    let callback = iced_layershell::actions::ActionCallback::new(move |region| {
        region.add(0, 0, width, height);
    });
    Task::done(Message::SetInputRegion {
        id: window,
        callback,
    })
}

pub fn app_subscription(_state: &BarState, gauges: &[String]) -> Subscription<Message> {
    let subs = vec![
        sway_workspace::workspace_subscription(),
        event::listen().map(Message::IcedEvent),
        window::open_events().map(Message::WindowOpened),
        window::events().map(|(id, event)| Message::WindowEvent(id, event)),
        window::close_events().map(Message::WindowClosed),
        gauge_work_manager::subscription(gauges),
    ];
    Subscription::batch(subs)
}

pub fn update(state: &mut BarState, message: Message) -> Task<Message> {
    let is_click_message = matches!(
        message,
        Message::WorkspaceClicked(_)
            | Message::WorkspaceAppClicked { .. }
            | Message::TopAppClicked { .. }
            | Message::BackgroundClicked
            | Message::GaugeClicked { .. }
            | Message::MenuItemSelected { .. }
            | Message::ActionItemSelected { .. }
    );
    if is_click_message && !state.allow_click() {
        return Task::none();
    }

    match message {
        Message::Workspaces { workspaces, apps } => {
            panels::ws_panel::update_workspace_focus(state, &workspaces);
            state.workspaces = workspaces;
            state.workspace_apps = apps
                .into_iter()
                .map(|entry| (entry.name, entry.apps))
                .collect();
        }
        Message::WorkspaceClicked(name) => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
            if let Err(err) = sway_workspace::focus_workspace(&name) {
                error!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::WorkspaceAppClicked { con_id, app_id } => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
            if let Err(err) = sway_workspace::focus_con_id(con_id) {
                error!("Failed to focus app \"{app_id}\" (con_id {con_id}): {err}");
            }
        }
        Message::TopAppClicked { app_id } => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
            if let Err(err) = sway_workspace::launch_app(&app_id) {
                error!("Failed to launch app \"{app_id}\": {err}");
                return Task::none();
            }
            if let Some(app) = state.top_apps.iter().find(|app| app.appid == app_id) {
                let mut cache = Cache::new(apps::load_desktop_apps);
                if let Err(err) = cache.record_launch(app) {
                    error!("Failed to update app cache for \"{app_id}\": {err}");
                }
                let top_apps_count =
                    settings::settings().get_parsed_or("grelier.app.top_apps.count", 6usize);
                state.top_apps = cache.top_apps(top_apps_count).unwrap_or_default();
            }
        }
        Message::IcedEvent(iced::Event::Mouse(mouse::Event::CursorMoved { position })) => {
            state.last_cursor = Some(position);
        }
        Message::BackgroundClicked => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
        }
        Message::IcedEvent(iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        })) => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
        }
        Message::GaugeBatch(batch) => {
            apply_gauge_batch(&mut state.gauges, &mut state.dialog_windows, batch);
        }
        Message::GaugeClicked { id, input } => {
            // If any dialog is open, any click just dismisses it.
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }

            let (gauge_menu, gauge_action, gauge_info, gauge_callback) =
                match state.gauges.iter().find(|g| g.id == id) {
                    Some(gauge) => (
                        gauge.menu.clone(),
                        gauge.action_dialog.clone(),
                        gauge.info.clone(),
                        gauge.on_click.clone(),
                    ),
                    None => (None, None, None, None),
                };

            if matches!(input, GaugeInput::Button(iced::mouse::Button::Right))
                && let Some(dialog) = gauge_action
            {
                let anchor_y = state
                    .gauge_dialog_anchor
                    .get(&id)
                    .copied()
                    .or_else(|| panels::gauge_panel::anchor_y(state));
                return state.open_action_dialog(&id, dialog, anchor_y);
            }

            if matches!(input, GaugeInput::Button(iced::mouse::Button::Right))
                && let Some(menu) = gauge_menu
            {
                let anchor_y = state
                    .gauge_dialog_anchor
                    .get(&id)
                    .copied()
                    .or_else(|| panels::gauge_panel::anchor_y(state));
                return state.open_menu(&id, menu, anchor_y);
            }

            if matches!(input, GaugeInput::Button(iced::mouse::Button::Left))
                && matches!(
                    id.as_str(),
                    "battery"
                        | "audio_in"
                        | "audio_out"
                        | "brightness"
                        | "cpu"
                        | "disk"
                        | "net_down"
                        | "net_up"
                        | "ram"
                        | "session"
                        | "wifi"
                )
                && let Some(dialog) = gauge_info
            {
                let anchor_y = state
                    .gauge_dialog_anchor
                    .get(&id)
                    .copied()
                    .or_else(|| panels::gauge_panel::anchor_y(state));
                return state.open_info_dialog(&id, dialog, anchor_y);
            }

            if let Some(callback) = gauge_callback {
                callback(GaugeClick { input });
            } else {
                info!("Gauge '{id}' clicked: {:?}", input);
            }
        }
        Message::MenuItemSelected {
            window,
            gauge_id,
            item_id,
        } => {
            // close menus first so clicking in parent bar after selection behaves consistently
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
            let _ = state.close_dialogs();
            if let Some(menu) = state
                .gauges
                .iter()
                .find(|g| g.id == gauge_id)
                .and_then(|g| g.menu.as_ref())
                .and_then(|menu| menu.on_select.clone())
            {
                menu(item_id);
            }
            return close_window_task(window);
        }
        Message::ActionItemSelected {
            window,
            gauge_id,
            item_id,
        } => {
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
            let _ = state.close_dialogs();
            if let Some(action) = state
                .gauges
                .iter()
                .find(|g| g.id == gauge_id)
                .and_then(|g| g.action_dialog.as_ref())
                .and_then(|dialog| dialog.on_select.clone())
            {
                action(item_id.clone());
            }
            return Task::done(Message::RemoveWindow(window));
        }
        Message::MenuItemHoverEnter { window, item_id } => {
            if let Some(dialog_window) = state.dialog_windows.get_mut(&window) {
                dialog_window.hovered_item = Some(item_id);
            }
        }
        Message::MenuItemHoverExit { window, item_id } => {
            if let Some(dialog_window) = state.dialog_windows.get_mut(&window)
                && dialog_window
                    .hovered_item
                    .as_ref()
                    .is_some_and(|hovered| hovered == &item_id)
            {
                dialog_window.hovered_item = None;
            }
        }
        Message::WindowFocusChanged { focused } => {
            return handle_window_focus_change(state, focused);
        }
        Message::WindowOpened(window) => {
            if let Some(task) = track_bar_window(state, window) {
                return task;
            }
        }
        Message::WindowEvent(window, event) => {
            if let iced::window::Event::Opened { size, .. } = event {
                let mut tasks = vec![set_input_region_task(window, size)];
                if let Some(task) = track_bar_window(state, window) {
                    tasks.push(task);
                }
                return Task::batch(tasks);
            }
            if event != iced::window::Event::Closed
                && let Some(task) = track_bar_window(state, window)
            {
                return task;
            }
        }
        Message::MenuDismissed(window) => {
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
            return close_window_task(window);
        }
        Message::CacheRefreshed(result) => match result {
            Ok((apps, top_apps)) => {
                let settings = settings::settings();
                let workspace_app_icons =
                    settings.get_bool_or("grelier.app.workspace.app_icons", true);
                state.app_icons = if workspace_app_icons {
                    AppIconCache::from_app_descriptors_ref(&apps)
                } else {
                    AppIconCache::default()
                };
                state.top_apps = top_apps;
            }
            Err(err) => {
                error!("Failed to refresh icon cache: {err}");
            }
        },
        Message::WindowClosed(window) => {
            let is_primary = state
                .primary_window
                .is_some_and(|primary| primary == window);
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
            state.bar_windows.remove(&window);
            if is_primary {
                if let Some(next_primary) = state.bar_windows.iter().copied().next() {
                    state.primary_window = Some(next_primary);
                    state.pending_primary_window = false;
                    return Task::none();
                }
                state.primary_window = None;
                state.pending_primary_window = true;
                let mut tasks = vec![state.close_dialogs()];
                let id = window::Id::unique();
                let task = Task::done(Message::NewLayerShell {
                    settings: layershell_reopen_settings(),
                    id,
                });
                tasks.push(Task::done(Message::ForgetLastOutput));
                tasks.push(task);
                return Task::batch(tasks);
            }
        }
        Message::OutputChanged => {
            if let Some(snapshot) = monitor::snapshot_outputs() {
                if !monitor::has_active_outputs(&snapshot) {
                    state.last_outputs = Some(snapshot);
                    return Task::none();
                }
                match state.last_outputs.as_ref() {
                    None => {
                        state.last_outputs = Some(snapshot);
                        return Task::none();
                    }
                    Some(prev) => {
                        if !monitor::has_active_outputs(prev) {
                            state.last_outputs = Some(snapshot);
                            return Task::none();
                        }
                        if monitor::outputs_equal(prev, &snapshot) {
                            state.last_outputs = Some(snapshot);
                            return Task::none();
                        }
                        state.last_outputs = Some(snapshot);
                    }
                }
            }
            let now = Instant::now();
            let reopened_since_last_output = state
                .last_output_change_at
                .and_then(|last| state.last_bar_window_opened_at.map(|opened| opened > last))
                .unwrap_or(false);
            if reopened_since_last_output {
                state.last_output_change_at = Some(now);
                return Task::none();
            }
            let recent_open = state
                .last_bar_window_opened_at
                .and_then(|last| now.checked_duration_since(last))
                .is_some_and(|elapsed| elapsed < OUTPUT_REOPEN_SUPPRESSION_WINDOW);
            if recent_open {
                return Task::none();
            }
            let recently_handled = state
                .last_output_change_at
                .and_then(|last| now.checked_duration_since(last))
                .is_some_and(|elapsed| elapsed < OUTPUT_REOPEN_SUPPRESSION_WINDOW);
            if recently_handled {
                return Task::none();
            }
            if state.bar_windows.len() > 1 {
                state.last_output_change_at = Some(now);
                return Task::none();
            }
            if state.pending_primary_window && state.primary_window.is_none() {
                return Task::none();
            }
            if state.primary_window.is_none() {
                return Task::none();
            }
            state.last_output_change_at = Some(now);
            // After resume/hotplug, the existing surface can go blank. Recreate the
            // primary window while ensuring we do not leave duplicates behind.
            return reopen_primary_window(state);
        }
        Message::IcedEvent(iced::Event::Window(iced::window::Event::Unfocused)) => {
            return Task::done(Message::WindowFocusChanged { focused: false });
        }
        Message::IcedEvent(_) => {}
        Message::NewLayerShell { id, .. } => {
            if let Some(task) = track_bar_window(state, id) {
                return task;
            }
        }
        Message::NewBaseWindow { id, .. } => {
            if let Some(task) = track_bar_window(state, id) {
                return task;
            }
        }
        Message::AnchorChange { .. }
        | Message::SetInputRegion { .. }
        | Message::AnchorSizeChange { .. }
        | Message::LayerChange { .. }
        | Message::MarginChange { .. }
        | Message::SizeChange { .. }
        | Message::ExclusiveZoneChange { .. }
        | Message::VirtualKeyboardPressed { .. }
        | Message::NewPopUp { .. }
        | Message::NewMenu { .. }
        | Message::NewInputPanel { .. }
        | Message::RemoveWindow(_)
        | Message::ForgetLastOutput => {}
    }

    Task::none()
}

fn track_bar_window(state: &mut BarState, window: window::Id) -> Option<Task<Message>> {
    if state.dialog_windows.contains_key(&window) || state.closing_dialogs.contains(&window) {
        return None;
    }

    let inserted = state.bar_windows.insert(window);
    if inserted {
        state.last_bar_window_opened_at = Some(Instant::now());
    }
    if state.primary_window.is_none() {
        state.primary_window = Some(window);
        state.pending_primary_window = false;
    }

    None
}

fn layershell_reopen_settings() -> NewLayerShellSettings {
    let settings = settings::settings();
    let bar_width = settings.get_parsed_or("grelier.bar.width", 28u32);
    let orientation_raw = settings.get_or("grelier.bar.orientation", DEFAULT_ORIENTATION);
    let orientation = match orientation_raw.parse::<Orientation>() {
        Ok(value) => value,
        Err(err) => {
            warn!("{err}; defaulting to {DEFAULT_ORIENTATION}");
            Orientation::Left
        }
    };
    let anchor = match orientation {
        Orientation::Left => Anchor::Left,
        Orientation::Right => Anchor::Right,
    };

    NewLayerShellSettings {
        size: Some((bar_width, 0)),
        layer: Layer::Top,
        anchor,
        exclusive_zone: Some(bar_width as i32),
        margin: Some((0, 0, 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::None,
        events_transparent: false,
        namespace: Some(BarState::namespace()),
    }
}

fn reopen_primary_window(state: &mut BarState) -> Task<Message> {
    state.pending_primary_window = true;
    state.primary_window = None;
    let closing_bar_windows: Vec<window::Id> = state.bar_windows.drain().collect();
    state
        .closing_dialogs
        .extend(closing_bar_windows.iter().copied());

    Task::batch(
        std::iter::once(state.close_dialogs())
            .chain(closing_bar_windows.into_iter().map(close_window_task))
            .chain(std::iter::once(Task::done(Message::ForgetLastOutput)))
            .chain(std::iter::once(Task::done(Message::NewLayerShell {
                settings: layershell_reopen_settings(),
                id: window::Id::unique(),
            }))),
    )
}

fn handle_window_focus_change(state: &mut BarState, focused: bool) -> Task<Message> {
    // Keep dialogs open when the bar regains focus.
    if focused {
        return Task::none();
    }

    // Ignore transient unfocus events immediately after opening a dialog.
    let recently_opened_dialog = state
        .last_dialog_opened_at
        .and_then(|last| Instant::now().checked_duration_since(last))
        .is_some_and(|elapsed| elapsed < DIALOG_UNFOCUS_SUPPRESSION_WINDOW);
    if recently_opened_dialog {
        return Task::none();
    }

    // Close the first tracked dialog on a real unfocus transition.
    if let Some(window) = state.dialog_windows.keys().copied().next() {
        state.dialog_windows.remove(&window);
        state.closing_dialogs.insert(window);
        return close_window_task(window);
    }

    // Nothing to close when no dialog windows are active.
    Task::none()
}

fn update_gauge(gauges: &mut Vec<GaugeModel>, new: GaugeModel) {
    if let Some(existing) = gauges.iter_mut().find(|g| g.id == new.id) {
        *existing = new;
    } else {
        gauges.push(new);
    }
}

fn apply_gauge_batch(
    gauges: &mut Vec<GaugeModel>,
    dialog_windows: &mut std::collections::HashMap<window::Id, GaugeDialogWindow>,
    batch: Vec<GaugeModel>,
) {
    for gauge in batch {
        refresh_info_dialogs(dialog_windows, &gauge);
        update_gauge(gauges, gauge);
    }
}

fn refresh_info_dialogs(
    dialog_windows: &mut std::collections::HashMap<window::Id, GaugeDialogWindow>,
    gauge: &GaugeModel,
) {
    let Some(info) = gauge.info.as_ref() else {
        return;
    };

    for dialog_window in dialog_windows.values_mut() {
        if dialog_window.gauge_id == gauge.id
            && let GaugeDialog::Info(dialog) = &mut dialog_window.dialog
        {
            *dialog = info.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bar::{DEFAULT_PANELS, GaugeDialog, GaugeDialogWindow};
    use crate::panels::gauges::gauge::{GaugeDisplay, GaugeMenu, GaugeValue, GaugeValueAttention};
    use crate::panels::gauges::gauge_registry;
    use crate::settings_storage::SettingsStorage;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn temp_storage_path(name: &str) -> (SettingsStorage, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("grelier_main_settings_test_{}", name));
        path.push(format!("Settings-{}.xresources", env!("CARGO_PKG_VERSION")));
        (SettingsStorage::new(path.clone()), path)
    }

    #[test]
    fn command_line_overrides_apply_before_settings_persist() {
        let (storage, path) = temp_storage_path("overrides_before_save");
        let settings_store = settings::Settings::new(storage.clone());

        settings_store.update("grelier.bar.theme", "Light");

        let mut all_setting_specs = Vec::new();
        let base_setting_specs = settings::base_setting_specs(
            gauge_registry::default_gauges(),
            DEFAULT_PANELS,
            DEFAULT_ORIENTATION,
            "Nord",
        );
        all_setting_specs.extend_from_slice(&base_setting_specs);
        let clock_spec = gauge_registry::find("clock").expect("clock gauge spec registered");
        all_setting_specs.extend_from_slice((clock_spec.settings)());
        settings_store.ensure_defaults(&all_setting_specs);

        let contents = std::fs::read_to_string(&path).expect("read settings storage");
        assert!(
            contents.contains("grelier.bar.theme: Light"),
            "expected override to persist before defaults"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn update_gauge_replaces_by_id() {
        let mut gauges = Vec::new();
        let g1 = GaugeModel {
            id: "clock",
            icon: None,
            display: GaugeDisplay::Value {
                value: GaugeValue::Text("12\n00".to_string()),
                attention: GaugeValueAttention::Nominal,
            },
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            display: GaugeDisplay::Value {
                value: GaugeValue::Text("12\n01".to_string()),
                attention: GaugeValueAttention::Nominal,
            },
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        };

        update_gauge(&mut gauges, g1.clone());
        assert_eq!(gauges.len(), 1);
        assert_text_value(&gauges[0], "12\n00");

        update_gauge(&mut gauges, g2.clone());
        assert_eq!(gauges.len(), 1, "should replace existing entry");
        assert_text_value(&gauges[0], "12\n01");

        let g3 = GaugeModel {
            id: "date",
            icon: None,
            display: GaugeDisplay::Value {
                value: GaugeValue::Text("01\n01".to_string()),
                attention: GaugeValueAttention::Nominal,
            },
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        };
        update_gauge(&mut gauges, g3.clone());
        assert_eq!(gauges.len(), 2, "different id should append");
    }

    #[test]
    fn left_click_closes_open_dialog_without_invoking_callback() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );

        let clicked = Arc::new(AtomicBool::new(false));
        state.gauges.push(GaugeModel {
            id: "audio_out",
            icon: None,
            display: GaugeDisplay::Empty,
            on_click: Some(Arc::new({
                let clicked = clicked.clone();
                move |_click| clicked.store(true, Ordering::SeqCst)
            })),
            menu: None,
            action_dialog: None,
            info: None,
        });

        let task = update(
            &mut state,
            Message::GaugeClicked {
                id: "audio_out".to_string(),
                input: GaugeInput::Button(mouse::Button::Left),
            },
        );

        assert!(
            !clicked.load(Ordering::SeqCst),
            "callback should not be invoked while closing dialog"
        );
        assert!(
            state.dialog_windows.is_empty(),
            "menu windows should be cleared"
        );
        assert!(
            state.closing_dialogs.contains(&window),
            "window should be marked for closing"
        );
        assert!(
            task.units() > 0,
            "closing menus should return a non-empty task"
        );
    }

    #[test]
    fn right_click_leaves_menu_open() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );
        state.gauges.push(GaugeModel {
            id: "audio_out",
            icon: None,
            display: GaugeDisplay::Empty,
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        });

        let task = update(
            &mut state,
            Message::GaugeClicked {
                id: "audio_out".to_string(),
                input: GaugeInput::Button(mouse::Button::Right),
            },
        );

        assert!(
            !state.dialog_windows.contains_key(&window),
            "any click should close existing menu"
        );
        assert!(
            state.closing_dialogs.contains(&window),
            "window should be marked for closing"
        );
        assert!(
            task.units() > 0,
            "close menus task should be returned even on right click"
        );
    }

    #[test]
    fn menu_item_selected_invokes_callback_and_closes_other_menus() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        let other_window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );
        state.dialog_windows.insert(
            other_window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Other".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );

        let selected = Arc::new(Mutex::new(None::<String>));
        let on_select = {
            let selected = Arc::clone(&selected);
            Arc::new(move |item: String| {
                *selected.lock().unwrap() = Some(item);
            })
        };
        state.gauges.push(GaugeModel {
            id: "audio_out",
            icon: None,
            display: GaugeDisplay::Empty,
            on_click: None,
            menu: Some(GaugeMenu {
                title: "Test".into(),
                items: Vec::new(),
                on_select: Some(on_select),
            }),
            action_dialog: None,
            info: None,
        });

        let task = update(
            &mut state,
            Message::MenuItemSelected {
                window,
                gauge_id: "audio_out".to_string(),
                item_id: "sink-1".to_string(),
            },
        );

        assert_eq!(
            selected.lock().unwrap().as_deref(),
            Some("sink-1"),
            "menu selection should be forwarded"
        );
        assert!(state.dialog_windows.is_empty(), "menus should be cleared");
        assert!(
            state.closing_dialogs.contains(&other_window),
            "other menus should be marked for closing"
        );
        assert!(
            !state.closing_dialogs.contains(&window),
            "selected window is closed directly"
        );
        assert!(task.units() > 0, "menu selection returns a close task");
    }

    #[test]
    fn menu_dismissed_clears_tracking() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );
        state.closing_dialogs.insert(window);

        let _ = update(&mut state, Message::MenuDismissed(window));

        assert!(
            !state.dialog_windows.contains_key(&window),
            "menu should be removed"
        );
        assert!(
            !state.closing_dialogs.contains(&window),
            "closing set should be cleared"
        );
    }

    #[test]
    fn window_unfocus_can_be_injected_for_tests() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "audio_out".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );
        state.last_dialog_opened_at = Some(Instant::now());

        let task = update(&mut state, Message::WindowFocusChanged { focused: false });

        assert!(
            state.dialog_windows.contains_key(&window),
            "recently opened dialog should remain visible"
        );
        assert_eq!(task.units(), 0, "suppressed unfocus should do nothing");
    }

    #[test]
    fn gauge_click_closes_existing_dialog_without_reopening() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: "test".to_string(),
                dialog: GaugeDialog::Menu(GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                }),
                hovered_item: None,
            },
        );

        let task = update(
            &mut state,
            Message::GaugeClicked {
                id: "test".to_string(),
                input: GaugeInput::Button(mouse::Button::Middle),
            },
        );

        assert!(
            state.dialog_windows.is_empty(),
            "dialog windows should be cleared on any click"
        );
        assert!(
            state.closing_dialogs.contains(&window),
            "existing dialog should be marked for closing"
        );
        assert!(task.units() > 0, "closing task should be returned");
    }

    #[test]
    fn track_bar_window_keeps_existing_primary_and_preserves_windows() {
        let mut state = BarState::default();
        let old_primary = window::Id::unique();
        let new_primary = window::Id::unique();
        state.primary_window = Some(old_primary);
        state.bar_windows.insert(old_primary);

        let task = track_bar_window(&mut state, new_primary);

        assert!(task.is_none(), "tracking bars should not queue closes");
        assert_eq!(state.primary_window, Some(old_primary));
        assert!(state.closing_dialogs.is_empty());
        assert_eq!(
            state.bar_windows.len(),
            2,
            "both windows should remain tracked"
        );
        assert!(state.bar_windows.contains(&old_primary));
        assert!(state.bar_windows.contains(&new_primary));
    }

    #[test]
    fn window_closed_promotes_remaining_bar_to_primary_without_reopen() {
        let mut state = BarState::default();
        let old_primary = window::Id::unique();
        let other = window::Id::unique();
        state.primary_window = Some(old_primary);
        state.bar_windows.insert(old_primary);
        state.bar_windows.insert(other);

        let task = update(&mut state, Message::WindowClosed(old_primary));

        assert_eq!(
            task.units(),
            0,
            "closing one bar should not reopen when another remains"
        );
        assert_eq!(state.primary_window, Some(other));
        assert!(!state.pending_primary_window);
        assert_eq!(state.bar_windows.len(), 1);
        assert!(state.bar_windows.contains(&other));
    }

    fn assert_text_value(model: &GaugeModel, expected: &str) {
        match &model.display {
            GaugeDisplay::Value {
                value: GaugeValue::Text(text),
                ..
            } => assert_eq!(text, expected),
            GaugeDisplay::Value {
                value: GaugeValue::Svg(_),
                ..
            } => panic!("expected text gauge value"),
            _ => panic!("expected value"),
        }
    }
}
