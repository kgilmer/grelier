// Entry point wiring CLI args, settings initialization, and gauge subscriptions for the bar.
// Consumes Settings: grelier.bar.width.
#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod gauges {
    pub mod audio_in;
    pub mod audio_out;
    pub mod battery;
    pub mod brightness;
    pub mod clock;
    pub mod cpu;
    pub mod date;
    pub mod disk;
    pub mod net_common;
    pub mod net_down;
    pub mod net_up;
    pub mod ram;
    pub mod test_gauge;
    pub mod wifi;
}
mod gauge;
mod icon;
mod menu_dialog;
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

use crate::app::Orientation;
use crate::app::{BarState, Message};
use crate::gauge::{GaugeClick, GaugeInput, GaugeModel, SettingSpec};
use crate::gauges::{
    audio_in, audio_out, battery, brightness, clock, cpu, date, disk, net_down, net_up, ram,
    test_gauge, wifi,
};

const DEFAULT_GAUGES: &str = "clock,date";
const DEFAULT_ORIENTATION: &str = "left";
const DEFAULT_THEME: &str = "Nord";

const BASE_SETTING_SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "grelier.gauges",
        default: DEFAULT_GAUGES,
    },
    SettingSpec {
        key: "grelier.orientation",
        default: DEFAULT_ORIENTATION,
    },
    SettingSpec {
        key: "grelier.theme",
        default: DEFAULT_THEME,
    },
];

#[derive(FromArgs, Debug)]
/// Workspace + gauges display
struct Args {
    /// comma-separated settings overrides (key=value,key2=value2)
    #[argh(option)]
    settings: Option<String>,

    /// list settings for the selected gauges and exit
    #[argh(switch)]
    list_settings: bool,
}

fn app_subscription(_state: &BarState, gauges: &[&str]) -> Subscription<Message> {
    let mut subs = Vec::new();
    subs.push(sway_workspace::workspace_subscription());
    subs.push(event::listen().map(Message::IcedEvent));
    subs.push(window::close_events().map(Message::WindowClosed));
    for gauge in gauges {
        match *gauge {
            "clock" => subs.push(clock::clock_subscription()),
            "date" => subs.push(date::date_subscription()),
            "battery" => subs.push(battery::battery_subscription()),
            "cpu" => subs.push(cpu::cpu_subscription()),
            "disk" => subs.push(disk::disk_subscription()),
            "net_down" => subs.push(net_down::net_down_subscription()),
            "net_up" => subs.push(net_up::net_up_subscription()),
            "ram" => subs.push(ram::ram_subscription()),
            "test_gauge" => subs.push(test_gauge::test_gauge_subscription()), // Special test gauge, intentionally omitted from help text
            "audio_out" => subs.push(audio_out::audio_out_subscription()),
            "brightness" => subs.push(brightness::brightness_subscription()),
            "audio_in" => subs.push(audio_in::audio_in_subscription()),
            "wifi" => subs.push(wifi::wifi_subscription()),
            other => unreachable!("gauges validated before subscription: {other}"),
        }
    }
    Subscription::batch(subs)
}

fn gauge_settings(gauge: &str) -> &'static [SettingSpec] {
    match gauge {
        "clock" => clock::settings(),
        "date" => date::settings(),
        "battery" => battery::settings(),
        "cpu" => cpu::settings(),
        "disk" => disk::settings(),
        "ram" => ram::settings(),
        "net_up" => net_up::settings(),
        "net_down" => net_down::settings(),
        "audio_out" => audio_out::settings(),
        "audio_in" => audio_in::settings(),
        "brightness" => brightness::settings(),
        "wifi" => wifi::settings(),
        "test_gauge" => test_gauge::settings(),
        other => unreachable!("gauges validated before settings list: {other}"),
    }
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    const KNOWN_GAUGES: &[&str] = &[
        "clock",
        "date",
        "battery",
        "cpu",
        "disk",
        "ram",
        "net_up",
        "net_down",
        "audio_out",
        "audio_in",
        "brightness",
        "wifi",
        "test_gauge",
    ];

    let storage =
        settings_storage::SettingsStorage::new(settings_storage::SettingsStorage::default_path());
    let settings_store = settings::init_settings(settings::Settings::new(storage));

    if let Some(arg) = args.settings.as_deref() {
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

    let mut all_setting_specs = Vec::new();
    all_setting_specs.extend_from_slice(BASE_SETTING_SPECS);
    for gauge in KNOWN_GAUGES {
        all_setting_specs.extend_from_slice(gauge_settings(gauge));
    }
    settings_store.ensure_defaults(&all_setting_specs);

    let gauges_setting = settings_store.get_or("grelier.gauges", DEFAULT_GAUGES);
    let gauges: Vec<String> = gauges_setting
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    for gauge in &gauges {
        if !KNOWN_GAUGES.contains(&gauge.as_str()) {
            eprintln!(
                "Unknown gauge '{gauge}'. Known gauges: {}",
                KNOWN_GAUGES.join(", ")
            );
            std::process::exit(1);
        }
    }

    if args.list_settings {
        for spec in BASE_SETTING_SPECS {
            println!("{}: {}", spec.key, spec.default);
        }

        for gauge in KNOWN_GAUGES {
            let specs = gauge_settings(gauge);

            for spec in specs {
                println!("{}:{}", spec.key, spec.default);
            }
        }

        return Ok(());
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
        .get_or("grelier.orientation", DEFAULT_ORIENTATION)
        .parse::<Orientation>()
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });

    let anchor = match orientation_setting {
        Orientation::Left => Anchor::Left,
        Orientation::Right => Anchor::Right,
    };

    let settings = LayerShellAppSettings {
        layer_settings: LayerShellSettings {
            size: Some((bar_width, 0)),
            exclusive_zone: bar_width as i32,
            anchor,
            layer: Layer::Top,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode: StartMode::Active,
            events_transparent: false,
        },
        antialiasing: true,
        default_font: Font::MONOSPACE,
        ..LayerShellAppSettings::default()
    };

    let theme = match settings_store.get("grelier.theme") {
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

    daemon(
        move || BarState::with_gauge_order(gauge_order.clone()),
        BarState::namespace,
        update,
        BarState::view,
    )
    .theme(theme)
    .subscription({
        let gauges = gauges.clone();
        move |state| {
            let gauge_refs: Vec<&str> = gauges.iter().map(|s| s.as_str()).collect();
            app_subscription(state, &gauge_refs)
        }
    })
    .settings(settings)
    .run()
}

fn update(state: &mut BarState, message: Message) -> Task<Message> {
    match message {
        Message::Workspaces(ws) => {
            state.update_workspace_focus(&ws);
            state.workspaces = ws;
        }
        Message::WorkspaceClicked(name) => {
            if !state.menu_windows.is_empty() {
                return state.close_menus();
            }
            if let Err(err) = sway_workspace::focus_workspace(&name) {
                eprintln!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::IcedEvent(iced::Event::Mouse(mouse::Event::CursorMoved { position })) => {
            state.last_cursor = Some(position);
        }
        Message::BackgroundClicked => {
            if !state.menu_windows.is_empty() {
                return state.close_menus();
            }
        }
        Message::Gauge(gauge) => {
            update_gauge(&mut state.gauges, gauge);
        }
        Message::GaugeClicked { id, target, input } => {
            let close_menus_task = if state.menu_windows.is_empty() {
                Task::none()
            } else {
                state.close_menus()
            };

            let gauge_index = state.gauges.iter().position(|g| g.id == id);
            let gauge_menu = gauge_index.and_then(|idx| state.gauges[idx].menu.clone());
            let gauge_callback = gauge_index.and_then(|idx| state.gauges[idx].on_click.clone());

            if matches!(input, GaugeInput::Button(iced::mouse::Button::Right))
                && let Some(menu) = gauge_menu
            {
                let anchor_y = state
                    .gauge_menu_anchor
                    .get(&id)
                    .copied()
                    .or_else(|| state.gauge_anchor_y(target));
                return Task::batch(vec![close_menus_task, state.open_menu(&id, menu, anchor_y)]);
            }

            if let Some(callback) = gauge_callback {
                callback(GaugeClick { input, target });
            } else {
                println!("Gauge '{id}' clicked: {:?} {:?}", target, input);
            }

            if close_menus_task.units() > 0 {
                return close_menus_task;
            }
        }
        Message::MenuItemSelected {
            window,
            gauge_id,
            item_id,
        } => {
            // close menus first so clicking in parent bar after selection behaves consistently
            state.menu_windows.remove(&window);
            state.closing_menus.remove(&window);
            let _ = state.close_menus();
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
        Message::MenuDismissed(window) => {
            state.menu_windows.remove(&window);
            state.closing_menus.remove(&window);
            return Task::done(Message::RemoveWindow(window));
        }
        Message::WindowClosed(window) => {
            state.menu_windows.remove(&window);
            state.closing_menus.remove(&window);
        }
        Message::IcedEvent(iced::Event::Window(iced::window::Event::Unfocused)) => {
            if let Some(window) = state.menu_windows.keys().copied().next() {
                state.menu_windows.remove(&window);
                return Task::done(Message::RemoveWindow(window));
            }
        }
        Message::IcedEvent(_) => {}
        Message::AnchorChange { .. }
        | Message::SetInputRegion { .. }
        | Message::AnchorSizeChange { .. }
        | Message::LayerChange { .. }
        | Message::MarginChange { .. }
        | Message::SizeChange { .. }
        | Message::ExclusiveZoneChange { .. }
        | Message::VirtualKeyboardPressed { .. }
        | Message::NewLayerShell { .. }
        | Message::NewBaseWindow { .. }
        | Message::NewPopUp { .. }
        | Message::NewMenu { .. }
        | Message::NewInputPanel { .. }
        | Message::RemoveWindow(_)
        | Message::ForgetLastOutput => {}
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

#[cfg(test)]
mod tests {
    use crate::settings_storage::SettingsStorage;
    use crate::app::GaugeMenuWindow;
    use crate::gauge::{GaugeClickTarget, GaugeMenu, GaugeValue, GaugeValueAttention};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn temp_storage_path(name: &str) -> (SettingsStorage, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("grelier_main_settings_test_{}", name));
        path.push("Settings.xresources");
        (SettingsStorage::new(path.clone()), path)
    }

    #[test]
    fn command_line_overrides_apply_before_settings_persist() {
        let (storage, path) = temp_storage_path("overrides_before_save");
        let settings_store = settings::Settings::new(storage.clone());

        settings_store.update("grelier.theme", "Light");

        let mut all_setting_specs = Vec::new();
        all_setting_specs.extend_from_slice(BASE_SETTING_SPECS);
        all_setting_specs.extend_from_slice(gauge_settings("clock"));
        settings_store.ensure_defaults(&all_setting_specs);

        let contents = std::fs::read_to_string(&path).expect("read settings storage");
        assert!(
            contents.contains("grelier.theme: Light"),
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
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            value: Some(GaugeValue::Text("12\n01".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
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
        };
        update_gauge(&mut gauges, g3.clone());
        assert_eq!(gauges.len(), 2, "different id should append");
    }

    #[test]
    fn left_click_closes_open_menu_and_invokes_callback() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.menu_windows.insert(
            window,
            GaugeMenuWindow {
                gauge_id: "audio_out".to_string(),
                menu: GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                },
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
        });

        let task = update(
            &mut state,
            Message::GaugeClicked {
                id: "audio_out".to_string(),
                target: GaugeClickTarget::Icon,
                input: GaugeInput::Button(mouse::Button::Left),
            },
        );

        assert!(clicked.load(Ordering::SeqCst), "callback should be invoked");
        assert!(
            state.menu_windows.is_empty(),
            "menu windows should be cleared"
        );
        assert!(
            state.closing_menus.contains(&window),
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
        state.menu_windows.insert(
            window,
            GaugeMenuWindow {
                gauge_id: "audio_out".to_string(),
                menu: GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                },
            },
        );
        state.gauges.push(GaugeModel {
            id: "audio_out",
            icon: None,
            value: None,
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
        });

        let task = update(
            &mut state,
            Message::GaugeClicked {
                id: "audio_out".to_string(),
                target: GaugeClickTarget::Icon,
                input: GaugeInput::Button(mouse::Button::Right),
            },
        );

        assert!(
            !state.menu_windows.contains_key(&window),
            "any click should close existing menu"
        );
        assert!(
            state.closing_menus.contains(&window),
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
        state.menu_windows.insert(
            window,
            GaugeMenuWindow {
                gauge_id: "audio_out".to_string(),
                menu: GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                },
            },
        );
        state.menu_windows.insert(
            other_window,
            GaugeMenuWindow {
                gauge_id: "audio_out".to_string(),
                menu: GaugeMenu {
                    title: "Other".into(),
                    items: Vec::new(),
                    on_select: None,
                },
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
        assert!(state.menu_windows.is_empty(), "menus should be cleared");
        assert!(
            state.closing_menus.contains(&other_window),
            "other menus should be marked for closing"
        );
        assert!(
            !state.closing_menus.contains(&window),
            "selected window is closed directly"
        );
        assert!(task.units() > 0, "menu selection returns a close task");
    }

    #[test]
    fn menu_dismissed_clears_tracking() {
        let mut state = BarState::default();
        let window = window::Id::unique();
        state.menu_windows.insert(
            window,
            GaugeMenuWindow {
                gauge_id: "audio_out".to_string(),
                menu: GaugeMenu {
                    title: "Test".into(),
                    items: Vec::new(),
                    on_select: None,
                },
            },
        );
        state.closing_menus.insert(window);

        let _ = update(&mut state, Message::MenuDismissed(window));

        assert!(
            !state.menu_windows.contains_key(&window),
            "menu should be removed"
        );
        assert!(
            !state.closing_menus.contains(&window),
            "closing set should be cleared"
        );
    }

    fn assert_text_value(model: &GaugeModel, expected: &str) {
        match &model.value {
            Some(GaugeValue::Text(text)) => assert_eq!(text, expected),
            Some(GaugeValue::Svg(_)) => panic!("expected text gauge value"),
            None => panic!("expected value"),
        }
    }
}
