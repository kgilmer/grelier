// Entry point wiring CLI args, settings initialization, and gauge subscriptions for the bar.
mod bar;
mod dialog_settings;
mod icon;
mod info_dialog;
mod menu_dialog;
mod panels;
mod settings;
mod settings_storage;
mod sway_workspace;
mod theme;

use argh::FromArgs;
use iced::Font;
use iced::Task;
use iced::{Subscription, event, mouse, window};

use iced_layershell::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings as LayerShellAppSettings, StartMode};

use crate::bar::Orientation;
use crate::bar::{AppIconCache, BarState, DEFAULT_PANELS, GaugeDialog, GaugeDialogWindow, Message};
use crate::panels::gauges::gauge::{GaugeClick, GaugeInput, GaugeModel};
use crate::panels::gauges::gauge_registry;
use elbey_cache::{AppDescriptor, Cache};
use freedesktop_desktop_entry::desktop_entries;
use locale_config::Locale;
use std::ffi::OsString;
use std::process::Command;
use std::time::{Duration, Instant};

const DEFAULT_ORIENTATION: &str = "left";
const DEFAULT_THEME: &str = "Nord";
const DIALOG_UNFOCUS_SUPPRESSION_WINDOW: Duration = Duration::from_millis(250);

#[derive(FromArgs, Debug)]
/// Workspace + gauges display
struct Args {
    /// setting override; repeat for multiple pairs (key=value or key:value)
    #[argh(option, short = 's', long = "settings")]
    setting: Vec<String>,

    /// list available themes and exit
    #[argh(switch)]
    list_themes: bool,

    /// list available gauges and exit
    #[argh(switch)]
    list_gauges: bool,

    /// list available panels and exit
    #[argh(switch)]
    list_panels: bool,

    /// override the settings file path
    #[argh(option, short = 'c', long = "config")]
    config: Option<std::path::PathBuf>,

    /// list app settings and exit
    #[argh(switch)]
    list_settings: bool,

    /// list available monitors and exit
    #[argh(switch)]
    list_monitors: bool,

    /// limit bar to specific monitors by name (comma-separated)
    #[argh(option, long = "on-monitors")]
    on_monitors: Option<String>,
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    if args.list_themes {
        theme::list_themes();
        return Ok(());
    }

    if args.list_gauges {
        gauge_registry::list_gauges();
        return Ok(());
    }

    if args.list_panels {
        bar::list_panels();
        return Ok(());
    }

    if args.list_monitors {
        list_monitors();
        return Ok(());
    }

    let mut monitor_names = args
        .on_monitors
        .as_deref()
        .map(parse_monitor_list)
        .unwrap_or_default();
    if args.on_monitors.is_some() {
        if monitor_names.is_empty() {
            eprintln!("--on-monitors requires at least one monitor name.");
            std::process::exit(1);
        }

        let outputs = match sway_workspace::fetch_outputs() {
            Ok(outputs) => outputs,
            Err(err) => {
                eprintln!("Failed to query outputs: {err}");
                std::process::exit(1);
            }
        };
        let known: std::collections::HashSet<String> =
            outputs.iter().map(|output| output.name.clone()).collect();
        monitor_names.retain(|name| !name.is_empty());
        let mut seen = std::collections::HashSet::new();
        let mut unique = Vec::new();
        for name in monitor_names.drain(..) {
            if seen.insert(name.clone()) {
                unique.push(name);
            }
        }
        monitor_names = unique;
        let unknown: Vec<String> = monitor_names
            .iter()
            .filter(|name| !known.contains(*name))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            eprintln!(
                "Unknown monitor(s): {}. Known monitors: {}",
                unknown.join(", "),
                known
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            std::process::exit(1);
        }
    }

    if monitor_names.len() > 1 {
        let exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(err) => {
                eprintln!("Failed to locate executable: {err}");
                std::process::exit(1);
            }
        };
        let forward_args = build_forward_args(&args);
        for name in &monitor_names {
            let mut cmd = Command::new(&exe);
            cmd.args(&forward_args);
            cmd.arg(format!("--on-monitors={name}"));
            if let Err(err) = cmd.spawn() {
                eprintln!("Failed to launch for monitor '{name}': {err}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let default_gauges = gauge_registry::default_gauges();
    let base_setting_specs = settings::base_setting_specs(
        default_gauges,
        DEFAULT_PANELS,
        DEFAULT_ORIENTATION,
        DEFAULT_THEME,
    );

    let mut registered_gauges: Vec<&'static gauge_registry::GaugeSpec> =
        gauge_registry::all().collect();
    registered_gauges.sort_by_key(|spec| spec.id);
    let known_gauge_names: Vec<&'static str> =
        registered_gauges.iter().map(|spec| spec.id).collect();
    let known_gauges: std::collections::HashSet<&'static str> =
        known_gauge_names.iter().copied().collect();

    let storage_path = args
        .config
        .clone()
        .unwrap_or_else(settings_storage::SettingsStorage::default_path);
    let storage = settings_storage::SettingsStorage::new(storage_path);
    let settings_store = settings::init_settings(settings::Settings::new(storage));

    for arg in &args.setting {
        let overrides = match settings::parse_settings_arg(arg) {
            Ok(map) => map,
            Err(err) => {
                eprintln!("Invalid settings: {err}");
                std::process::exit(1);
            }
        };
        for (key, value) in overrides {
            settings_store.update(&key, &value);
        }
    }

    let all_setting_specs = gauge_registry::collect_settings(&base_setting_specs);
    settings_store.ensure_defaults(&all_setting_specs);

    let gauges_setting = settings_store.get_or("grelier.gauges", default_gauges);
    let gauges: Vec<String> = gauges_setting
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    for gauge in &gauges {
        if !known_gauges.contains(gauge.as_str()) {
            eprintln!(
                "Unknown gauge '{gauge}'. Known gauges: {}",
                known_gauge_names.join(", ")
            );
            std::process::exit(1);
        }
    }

    if args.list_settings {
        gauge_registry::list_settings(&base_setting_specs);

        return Ok(());
    }

    if let Err(err) = gauge_registry::validate_settings(settings_store) {
        eprintln!("{err}");
        std::process::exit(1);
    }

    let mut known_settings = std::collections::HashSet::new();
    for spec in &all_setting_specs {
        if !known_settings.insert(spec.key) {
            eprintln!("Duplicate setting key '{}'", spec.key);
            std::process::exit(1);
        }
    }

    let bar_width = settings_store.get_parsed_or("grelier.bar.width", 28u32);

    let orientation_setting = settings_store
        .get_or("grelier.bar.orientation", DEFAULT_ORIENTATION)
        .parse::<Orientation>()
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });

    let anchor = match orientation_setting {
        Orientation::Left => Anchor::Left,
        Orientation::Right => Anchor::Right,
    };

    let start_mode = if let Some(name) = monitor_names.first() {
        StartMode::TargetScreen(name.clone())
    } else {
        StartMode::AllScreens
    };

    let settings = LayerShellAppSettings {
        layer_settings: LayerShellSettings {
            size: Some((bar_width, 0)),
            exclusive_zone: bar_width as i32,
            anchor,
            layer: Layer::Top,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode,
            events_transparent: false,
        },
        antialiasing: true,
        default_font: Font::MONOSPACE,
        ..LayerShellAppSettings::default()
    };

    let theme = match settings_store.get("grelier.bar.theme") {
        Some(name) => match theme::parse_them(&name) {
            Some(theme) => theme,
            None => {
                eprintln!(
                    "Unknown theme '{name}'. Valid themes: {}",
                    theme::VALID_THEME_NAMES.join(", ")
                );
                std::process::exit(1);
            }
        },
        None => theme::DEFAULT_THEME,
    };

    let gauge_order = gauges.clone();
    let workspace_app_icons = settings_store.get_bool_or("grelier.app.workspace.app_icons", true);
    let top_apps_count = settings_store.get_parsed_or("grelier.app.top_apps.count", 6usize);

    daemon(
        move || {
            let mut icon_cache = Cache::new(load_desktop_apps);
            let (mut apps, app_icons, top_apps) =
                load_cached_apps_from_cache(&mut icon_cache, top_apps_count, workspace_app_icons);
            let refresh_task = if workspace_app_icons || top_apps_count > 0 {
                Task::perform(
                    async move {
                        let top_apps = icon_cache
                            .refresh_with_top(&mut apps, top_apps_count)
                            .map_err(|err| err.to_string())?;
                        Ok((apps, top_apps))
                    },
                    Message::CacheRefreshed,
                )
            } else {
                Task::none()
            };
            (
                BarState::with_gauge_order_and_icons(gauge_order.clone(), app_icons, top_apps),
                refresh_task,
            )
        },
        BarState::namespace,
        update,
        BarState::view,
    )
    .theme(theme)
    .subscription({
        let gauges = gauges.clone();
        move |state| app_subscription(state, &gauges)
    })
    .settings(settings)
    .run()
}

fn load_desktop_apps() -> Vec<AppDescriptor> {
    let locales: Vec<String> = Locale::user_default()
        .tags()
        .map(|(_, tag)| tag.to_string())
        .collect();
    desktop_entries(&locales)
        .into_iter()
        .map(AppDescriptor::from)
        .collect()
}

fn load_cached_apps_from_cache(
    cache: &mut Cache,
    top_count: usize,
    workspace_app_icons: bool,
) -> (Vec<AppDescriptor>, AppIconCache, Vec<AppDescriptor>) {
    let apps = if workspace_app_icons || top_count > 0 {
        cache.load_apps()
    } else {
        Vec::new()
    };

    let app_icons = if workspace_app_icons {
        AppIconCache::from_app_descriptors_ref(&apps)
    } else {
        AppIconCache::default()
    };

    let top_apps = if top_count > 0 {
        cache
            .top_apps(top_count)
            .unwrap_or_default()
            .into_iter()
            .filter(|app| app.exec_count > 0)
            .collect()
    } else {
        Vec::new()
    };

    (apps, app_icons, top_apps)
}

fn parse_monitor_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .collect()
}

fn list_monitors() {
    match sway_workspace::fetch_outputs() {
        Ok(outputs) => {
            if outputs.is_empty() {
                println!("No outputs detected.");
                return;
            }
            for output in outputs {
                let status = if output.active { "active" } else { "inactive" };
                let make_model = format!("{} {}", output.make, output.model)
                    .trim()
                    .to_string();
                if make_model.trim().is_empty() {
                    println!("{}\t{}", output.name, status);
                } else {
                    println!("{}\t{}\t{}", output.name, status, make_model.trim());
                }
            }
        }
        Err(err) => {
            eprintln!("Failed to query outputs: {err}");
            std::process::exit(1);
        }
    }
}

fn build_forward_args(args: &Args) -> Vec<OsString> {
    let mut out = Vec::new();
    for setting in &args.setting {
        out.push(OsString::from("--settings"));
        out.push(OsString::from(setting));
    }
    if let Some(config) = &args.config {
        out.push(OsString::from("--config"));
        out.push(config.as_os_str().to_os_string());
    }
    out
}

fn app_subscription(_state: &BarState, gauges: &[String]) -> Subscription<Message> {
    let mut subs = Vec::new();
    subs.push(sway_workspace::workspace_subscription());
    subs.push(event::listen().map(Message::IcedEvent));
    subs.push(window::close_events().map(Message::WindowClosed));
    for gauge in gauges {
        if let Some(spec) = gauge_registry::find(gauge) {
            subs.push(gauge_registry::subscription_for(spec));
        } else {
            eprintln!("Unknown gauge '{gauge}' in subscription list.");
        }
    }
    Subscription::batch(subs)
}

fn update(state: &mut BarState, message: Message) -> Task<Message> {
    let is_click_message = matches!(
        message,
        Message::WorkspaceClicked(_)
            | Message::WorkspaceAppClicked { .. }
            | Message::TopAppClicked { .. }
            | Message::BackgroundClicked
            | Message::GaugeClicked { .. }
            | Message::MenuItemSelected { .. }
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
                eprintln!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::WorkspaceAppClicked { con_id, app_id } => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
            if let Err(err) = sway_workspace::focus_con_id(con_id) {
                eprintln!("Failed to focus app \"{app_id}\" (con_id {con_id}): {err}");
            }
        }
        Message::TopAppClicked { app_id } => {
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }
            if let Err(err) = sway_workspace::launch_app(&app_id) {
                eprintln!("Failed to launch app \"{app_id}\": {err}");
                return Task::none();
            }
            if let Some(app) = state.top_apps.iter().find(|app| app.appid == app_id) {
                let mut cache = Cache::new(load_desktop_apps);
                if let Err(err) = cache.record_launch(app) {
                    eprintln!("Failed to update app cache for \"{app_id}\": {err}");
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
        Message::Gauge(gauge) => {
            update_gauge(&mut state.gauges, gauge.clone());
            refresh_info_dialogs(&mut state.dialog_windows, &gauge);
        }
        Message::GaugeClicked { id, input } => {
            // If any dialog is open, any click just dismisses it.
            if !state.dialog_windows.is_empty() {
                return state.close_dialogs();
            }

            let (gauge_menu, gauge_info, gauge_callback) =
                match state.gauges.iter().find(|g| g.id == id) {
                    Some(gauge) => (
                        gauge.menu.clone(),
                        gauge.info.clone(),
                        gauge.on_click.clone(),
                    ),
                    None => (None, None, None),
                };

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
                println!("Gauge '{id}' clicked: {:?}", input);
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
                menu(item_id.clone());
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
        Message::MenuDismissed(window) => {
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
            return Task::done(Message::RemoveWindow(window));
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
                eprintln!("Failed to refresh icon cache: {err}");
            }
        },
        Message::WindowClosed(window) => {
            state.dialog_windows.remove(&window);
            state.closing_dialogs.remove(&window);
        }
        Message::IcedEvent(iced::Event::Window(iced::window::Event::Unfocused)) => {
            return Task::done(Message::WindowFocusChanged { focused: false });
        }
        Message::IcedEvent(_) => {}
        Message::NewLayerShell { id, .. } => {
            if state.primary_window.is_none() {
                state.primary_window = Some(id);
            }
        }
        Message::NewBaseWindow { id, .. } => {
            if state.primary_window.is_none() {
                state.primary_window = Some(id);
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

fn handle_window_focus_change(state: &mut BarState, focused: bool) -> Task<Message> {
    if focused {
        return Task::none();
    }

    let recently_opened_dialog = state
        .last_dialog_opened_at
        .and_then(|last| Instant::now().checked_duration_since(last))
        .is_some_and(|elapsed| elapsed < DIALOG_UNFOCUS_SUPPRESSION_WINDOW);
    if recently_opened_dialog {
        return Task::none();
    }

    if let Some(window) = state.dialog_windows.keys().copied().next() {
        state.dialog_windows.remove(&window);
        state.closing_dialogs.insert(window);
        return Task::done(Message::RemoveWindow(window));
    }

    Task::none()
}

fn update_gauge(gauges: &mut Vec<GaugeModel>, new: GaugeModel) {
    if let Some(existing) = gauges.iter_mut().find(|g| g.id == new.id) {
        *existing = new;
    } else {
        gauges.push(new);
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
    use crate::bar::{GaugeDialog, GaugeDialogWindow};
    use crate::panels::gauges::gauge::{GaugeMenu, GaugeValue, GaugeValueAttention};
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
            DEFAULT_THEME,
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
            value: Some(GaugeValue::Text("12\n00".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
            info: None,
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            value: Some(GaugeValue::Text("12\n01".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
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
            value: Some(GaugeValue::Text("01\n01".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
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
            value: None,
            attention: GaugeValueAttention::Nominal,
            on_click: Some(Arc::new({
                let clicked = clicked.clone();
                move |_click| clicked.store(true, Ordering::SeqCst)
            })),
            menu: None,
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
            value: None,
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
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
            value: None,
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: Some(GaugeMenu {
                title: "Test".into(),
                items: Vec::new(),
                on_select: Some(on_select),
            }),
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

    fn assert_text_value(model: &GaugeModel, expected: &str) {
        match &model.value {
            Some(GaugeValue::Text(text)) => assert_eq!(text, expected),
            Some(GaugeValue::Svg(_)) => panic!("expected text gauge value"),
            None => panic!("expected value"),
        }
    }
}
