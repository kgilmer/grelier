// Desktop session actions gauge with uptime info and session controls.
use crate::dialog::info::InfoDialog;
use crate::icon::svg_asset;
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{
    ActionSelectAction, GaugeActionDialog, GaugeActionItem, GaugeDisplay, GaugeModel,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings::SettingSpec;
use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy};

const LOGIND_SERVICE: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const LOGIND_IFACE: &str = "org.freedesktop.login1.Manager";
const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, Copy)]
enum SessionAction {
    Sleep,
    Reboot,
    Shutdown,
}

impl SessionAction {
    fn from_item_id(item_id: &str) -> Option<Self> {
        match item_id {
            "sleep" => Some(Self::Sleep),
            "reboot" => Some(Self::Reboot),
            "shutdown" => Some(Self::Shutdown),
            _ => None,
        }
    }
}

fn perform_session_action(action: SessionAction) {
    let connection = match Connection::system() {
        Ok(connection) => connection,
        Err(err) => {
            log::error!("session gauge: failed to connect to system bus: {err}");
            return;
        }
    };

    let proxy = match Proxy::new(&connection, LOGIND_SERVICE, LOGIND_PATH, LOGIND_IFACE) {
        Ok(proxy) => proxy,
        Err(err) => {
            log::error!("session gauge: failed to create logind proxy: {err}");
            return;
        }
    };

    let result = match action {
        SessionAction::Sleep => proxy.call_method("Suspend", &(false,)),
        SessionAction::Reboot => proxy.call_method("Reboot", &(false,)),
        SessionAction::Shutdown => proxy.call_method("PowerOff", &(false,)),
    };

    if let Err(err) = result {
        log::error!("session gauge: action failed: {err}");
    }
}

fn read_uptime_seconds() -> Option<u64> {
    let uptime = fs::read_to_string("/proc/uptime").ok()?;
    let first = uptime.split_whitespace().next()?;
    let seconds = first.parse::<f64>().ok()?;
    Some(seconds.max(0.0) as u64)
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours:02}h {minutes:02}m")
    } else {
        format!("{hours:02}h {minutes:02}m")
    }
}

fn session_action_dialog() -> GaugeActionDialog {
    let on_select: ActionSelectAction = Arc::new(|item_id: String| {
        let Some(action) = SessionAction::from_item_id(&item_id) else {
            log::warn!("session gauge: unknown action '{item_id}'");
            return;
        };
        thread::spawn(move || perform_session_action(action));
    });

    GaugeActionDialog {
        title: "Session".to_string(),
        items: vec![
            GaugeActionItem {
                id: "sleep".to_string(),
                icon: svg_asset("sleep.svg"),
            },
            GaugeActionItem {
                id: "reboot".to_string(),
                icon: svg_asset("reboot.svg"),
            },
            GaugeActionItem {
                id: "shutdown".to_string(),
                icon: svg_asset("shutdown.svg"),
            },
        ],
        on_select: Some(on_select),
    }
}

struct SessionGauge {
    action_dialog: GaugeActionDialog,
    next_deadline: Instant,
}

impl Gauge for SessionGauge {
    fn id(&self) -> &'static str {
        "session"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        self.next_deadline = now + Duration::from_secs(DEFAULT_REFRESH_INTERVAL_SECS);
        Some(GaugeModel {
            id: "session",
            icon: Some(svg_asset("shutdown.svg")),
            display: GaugeDisplay::Empty,
            on_click: None,
            menu: None,
            action_dialog: Some(self.action_dialog.clone()),
            info: Some(InfoDialog {
                title: "Session".to_string(),
                lines: vec![match read_uptime_seconds() {
                    Some(seconds) => format!("Uptime: {}", format_uptime(seconds)),
                    None => "Uptime: Unknown".to_string(),
                }],
            }),
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    Box::new(SessionGauge {
        action_dialog: session_action_dialog(),
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "session",
        description: "Desktop session actions with uptime info.",
        default_enabled: false,
        settings,
        create: create_gauge,
        validate: None,
    }
}
