use crate::app::Message;
use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeInput, GaugeModel, GaugeValue, GaugeValueAttention,
    SettingSpec, event_stream,
};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use crate::settings;
use iced::futures::StreamExt;
use iced::{Subscription, mouse};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc as sync_mpsc};
use std::time::Duration;

const SYS_NET: &str = "/sys/class/net";
const DEFAULT_QUALITY_MAX: f32 = 70.0;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 3;

#[derive(Clone, Copy, Debug)]
enum WifiState {
    Connected,
    NotConnected,
    NoDevice,
}

#[derive(Clone, Copy, Debug)]
struct WifiSnapshot {
    state: WifiState,
    strength: f32,
}

fn wifi_interfaces() -> Vec<String> {
    let mut ifaces = Vec::new();
    let entries = match fs::read_dir(SYS_NET) {
        Ok(entries) => entries,
        Err(_) => return ifaces,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if is_wifi_iface(&path)
            && let Some(name) = path.file_name().and_then(|s| s.to_str())
        {
            ifaces.push(name.to_string());
        }
    }

    ifaces
}

fn is_wifi_iface(path: &Path) -> bool {
    path.join("wireless").exists() || path.join("phy80211").exists()
}

fn read_carrier(path: &Path) -> Option<bool> {
    let value = fs::read_to_string(path.join("carrier")).ok()?;
    match value.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn read_operstate(path: &Path) -> Option<String> {
    fs::read_to_string(path.join("operstate"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_link_quality(iface: &str) -> Option<f32> {
    let contents = fs::read_to_string("/proc/net/wireless").ok()?;
    for line in contents.lines().skip(2) {
        let mut parts = line.split_whitespace();
        let name = parts.next()?.trim_end_matches(':');
        if name != iface {
            continue;
        }
        let _status = parts.next()?;
        let link = parts.next()?.trim_end_matches('.');
        return link.parse::<f32>().ok();
    }
    None
}

fn interface_connected(path: &Path, quality: Option<f32>) -> bool {
    if let Some(carrier) = read_carrier(path) {
        return carrier;
    }

    if let Some(operstate) = read_operstate(path)
        && operstate != "up"
    {
        return false;
    }

    quality.unwrap_or(0.0) > 0.0
}

fn pick_interface(ifaces: &[String]) -> Option<String> {
    for iface in ifaces {
        let path = PathBuf::from(SYS_NET).join(iface);
        let quality = read_link_quality(iface);
        if interface_connected(&path, quality) {
            return Some(iface.clone());
        }
    }

    ifaces.first().cloned()
}

fn wifi_snapshot(quality_max: f32) -> WifiSnapshot {
    let ifaces = wifi_interfaces();
    if ifaces.is_empty() {
        return WifiSnapshot {
            state: WifiState::NoDevice,
            strength: 0.0,
        };
    }

    let Some(iface) = pick_interface(&ifaces) else {
        return WifiSnapshot {
            state: WifiState::NoDevice,
            strength: 0.0,
        };
    };

    let path = PathBuf::from(SYS_NET).join(&iface);
    let quality = read_link_quality(&iface);
    let connected = interface_connected(&path, quality);
    let strength = quality.unwrap_or(0.0).clamp(0.0, quality_max) / quality_max;

    WifiSnapshot {
        state: if connected {
            WifiState::Connected
        } else {
            WifiState::NotConnected
        },
        strength,
    }
}

fn wifi_gauge(
    snapshot: WifiSnapshot,
    style: QuantityStyle,
    on_click: Option<GaugeClickAction>,
) -> GaugeModel {
    let (icon, attention) = match snapshot.state {
        WifiState::Connected => ("wifi.svg", GaugeValueAttention::Nominal),
        WifiState::NotConnected => ("wifi-off.svg", GaugeValueAttention::Warning),
        WifiState::NoDevice => ("wifi-no.svg", GaugeValueAttention::Danger),
    };

    GaugeModel {
        id: "wifi",
        icon: Some(svg_asset(icon)),
        value: match snapshot.state {
            WifiState::NoDevice => None,
            _ => Some(GaugeValue::Svg(icon_quantity(style, snapshot.strength))),
        },
        attention,
        on_click,
        menu: None,
    }
}

fn wifi_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("wifi", None, move |mut sender| {
        let style_value = settings::settings().get_or("grelier.wifi.quantitystyle", "grid");
        let style = QuantityStyle::parse_setting("grelier.wifi.quantitystyle", &style_value);
        let mut quality_max =
            settings::settings().get_parsed_or("grelier.wifi.quality_max", DEFAULT_QUALITY_MAX);
        if quality_max <= 0.0 {
            quality_max = DEFAULT_QUALITY_MAX;
        }
        let poll_interval_secs = settings::settings().get_parsed_or(
            "grelier.wifi.poll_interval_secs",
            DEFAULT_POLL_INTERVAL_SECS,
        );
        let poll_interval = Duration::from_secs(poll_interval_secs);
        let state = Arc::new(Mutex::new(style));
        let (trigger_tx, trigger_rx) = sync_mpsc::channel::<()>();
        let on_click: GaugeClickAction = {
            let state = Arc::clone(&state);
            let trigger_tx = trigger_tx.clone();
            Arc::new(move |click: GaugeClick| {
                if matches!(click.input, GaugeInput::Button(mouse::Button::Left)) {
                    if let Ok(mut style) = state.lock() {
                        *style = style.toggle();
                        settings::settings()
                            .update("grelier.wifi.quantitystyle", style.as_setting_value());
                    }
                    let _ = trigger_tx.send(());
                }
            })
        };

        let _trigger_tx = trigger_tx;
        loop {
            let snapshot = wifi_snapshot(quality_max);
            let style = state
                .lock()
                .map(|style| *style)
                .unwrap_or(QuantityStyle::Grid);
            let _ = sender.try_send(wifi_gauge(snapshot, style, Some(on_click.clone())));

            match trigger_rx.recv_timeout(poll_interval) {
                Ok(()) | Err(sync_mpsc::RecvTimeoutError::Timeout) => continue,
                Err(sync_mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    })
}

pub fn wifi_subscription() -> Subscription<Message> {
    Subscription::run(|| wifi_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.wifi.quantitystyle",
            default: "grid",
        },
        SettingSpec {
            key: "grelier.wifi.quality_max",
            default: "70",
        },
        SettingSpec {
            key: "grelier.wifi.poll_interval_secs",
            default: "3",
        },
    ];
    SETTINGS
}
