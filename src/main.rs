// Entry point wiring CLI args, settings initialization, and gauge subscriptions for the bar.

use argh::FromArgs;
use iced::Font;
use iced::Task;

use iced_layershell::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings as LayerShellAppSettings, StartMode};

use elbey_cache::Cache;
use grelier::apps;
use grelier::bar::{BarState, DEFAULT_PANELS, Message, Orientation};
use grelier::gauges::gauge_registry;
use grelier::monitor;
use grelier::runtime_dispatch::{app_subscription, update};
use grelier::settings;
use grelier::settings_storage;
use grelier::theme;
use log::{error, info, warn};
use std::io::Write;
use std::path::Path;

const DEFAULT_ORIENTATION: &str = "left";
const DEFAULT_THEME: &str = "Nord";

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut stderr = std::io::stderr();
        let _ = writeln!(stderr, "[{}] {}", record.level(), record.args());
    }

    fn flush(&self) {}
}

fn init_logging() {
    let level = std::env::var("GREL_LOG_LEVEL")
        .ok()
        .and_then(|value| value.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Warn);
    let formatter = syslog::Formatter3164 {
        facility: syslog::Facility::LOG_USER,
        hostname: None,
        process: "grelier".to_string(),
        pid: std::process::id(),
    };

    let (logger, syslog_error) = match syslog::unix(formatter) {
        Ok(logger) => (
            Box::new(syslog::BasicLogger::new(logger)) as Box<dyn log::Log>,
            None,
        ),
        Err(err) => (Box::new(StderrLogger) as Box<dyn log::Log>, Some(err)),
    };

    if log::set_boxed_logger(logger).is_ok() {
        log::set_max_level(level);
        if let Some(err) = syslog_error {
            warn!("Failed to connect to syslog; using stderr logger: {err}");
        }
    }
}

fn write_stderr(message: &str) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{message}");
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(location) = info.location() {
            format!("Panic at {}:{}: {}", location.file(), location.line(), info)
        } else {
            format!("Panic: {info}")
        };
        error!("{message}");
        write_stderr(&message);
    }));
}

fn exit_with_error(message: impl std::fmt::Display) -> ! {
    let message = message.to_string();
    error!("{message}");
    write_stderr(&message);
    info!("Exiting with status 1.");
    std::process::exit(1);
}

fn ensure_layershell_environment() -> Result<(), String> {
    let session_type = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.to_ascii_lowercase());
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();

    if wayland_display.is_none() {
        let mut message = String::from(
            "Wayland compositor not detected. grelier requires a Wayland session with layer-shell support.",
        );
        if matches!(session_type.as_deref(), Some("x11")) {
            message.push_str(" Current session is X11.");
        }
        message.push_str(
            " Start grelier from Sway (or another wlroots compositor that supports layer-shell).",
        );
        return Err(message);
    }

    let wayland_display = wayland_display
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "WAYLAND_DISPLAY is set but empty. Start grelier from a valid Wayland session."
                .to_string()
        })?;

    let runtime_dir = xdg_runtime_dir
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "XDG_RUNTIME_DIR is not set. Cannot locate Wayland socket; run grelier from a login session."
                .to_string()
        })?;

    let socket_path = Path::new(runtime_dir).join(wayland_display);
    if !socket_path.exists() {
        return Err(format!(
            "Wayland socket '{}' does not exist. Ensure your compositor is running and launch grelier inside that session.",
            socket_path.display()
        ));
    }

    Ok(())
}

#[derive(FromArgs, Debug)]
/// Grelier command line argument spec
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

    /// limit bar to one monitor by name
    #[argh(option, long = "on-monitor")]
    on_monitor: Option<String>,
}

fn main() -> Result<(), iced_layershell::Error> {
    init_logging();
    install_panic_hook();
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
        grelier::bar::list_panels();
        return Ok(());
    }

    if args.list_monitors {
        if let Err(err) = monitor::list_monitors() {
            exit_with_error(err);
        }
        return Ok(());
    }

    let monitor_name = monitor::normalize_monitor_selection(args.on_monitor.as_deref())
        .unwrap_or_else(|err| exit_with_error(err));

    if let Err(err) = ensure_layershell_environment() {
        exit_with_error(err);
    }

    let default_gauges = gauge_registry::default_gauges();
    let base_setting_specs = settings::base_setting_specs(
        default_gauges,
        DEFAULT_PANELS,
        DEFAULT_ORIENTATION,
        DEFAULT_THEME,
    );

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
                exit_with_error(format!("Invalid settings: {err}"));
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

    if args.list_settings {
        gauge_registry::list_settings(&base_setting_specs);

        return Ok(());
    }

    if let Err(err) = gauge_registry::validate_settings(settings_store) {
        exit_with_error(err);
    }

    let mut known_settings = std::collections::HashSet::new();
    for spec in &all_setting_specs {
        if !known_settings.insert(spec.key) {
            exit_with_error(format!("Duplicate setting key '{}'", spec.key));
        }
    }

    let bar_width = settings_store.get_parsed_or("grelier.bar.width", 28u32);

    let orientation_setting = settings_store
        .get_or("grelier.bar.orientation", DEFAULT_ORIENTATION)
        .parse::<Orientation>()
        .unwrap_or_else(|err| {
            exit_with_error(err);
        });

    let anchor = match orientation_setting {
        Orientation::Left => Anchor::Left,
        Orientation::Right => Anchor::Right,
    };

    let start_mode = match monitor_name {
        Some(name) => StartMode::TargetScreen(name),
        None => StartMode::AllScreens,
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
        Some(name) if theme::is_custom_theme_name(&name) => {
            theme::custom_theme_from_settings(settings_store).unwrap_or_else(|err| {
                exit_with_error(err);
            })
        }
        Some(name) => match theme::parse_them(&name) {
            Some(theme) => theme,
            None => {
                exit_with_error(format!(
                    "Unknown theme '{name}'. Valid themes: {}",
                    theme::VALID_THEME_NAMES.join(", ")
                ));
            }
        },
        None => theme::DEFAULT_THEME,
    };

    let gauge_order = gauges;
    let gauges_for_subscription = gauge_order.clone();
    let workspace_app_icons = settings_store.get_bool_or("grelier.app.workspace.app_icons", true);
    let top_apps_count = settings_store.get_parsed_or("grelier.app.top_apps.count", 6usize);

    let theme_for_state = theme.clone();
    let run_result = daemon(
        move || {
            let mut icon_cache = Cache::new(apps::load_desktop_apps);
            let (mut apps, app_icons, top_apps) = apps::load_cached_apps_from_cache(
                &mut icon_cache,
                top_apps_count,
                workspace_app_icons,
            );
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
                {
                    let mut state = BarState::with_gauge_order_and_icons(
                        gauge_order.clone(),
                        app_icons,
                        top_apps,
                    );
                    state.bar_theme = theme_for_state.clone();
                    state
                },
                refresh_task,
            )
        },
        BarState::namespace,
        update,
        BarState::view,
    )
    .theme(theme)
    .subscription(move |state| app_subscription(state, &gauges_for_subscription))
    .settings(settings)
    .run();

    match &run_result {
        Ok(()) => info!("Exiting normally after bar run completed."),
        Err(err) => error!("Exiting with error after bar run completed: {err}"),
    }
    run_result
}
