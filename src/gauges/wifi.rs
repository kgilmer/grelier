use crate::app::Message;
use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention, event_stream};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use iced::Subscription;
use iced::futures::StreamExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const SYS_NET: &str = "/sys/class/net";
const QUALITY_MAX: f32 = 70.0;
const POLL_INTERVAL: Duration = Duration::from_secs(3);

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

fn wifi_snapshot() -> WifiSnapshot {
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
    let strength = quality.unwrap_or(0.0).clamp(0.0, QUALITY_MAX) / QUALITY_MAX;

    WifiSnapshot {
        state: if connected {
            WifiState::Connected
        } else {
            WifiState::NotConnected
        },
        strength,
    }
}

fn wifi_gauge(snapshot: WifiSnapshot) -> GaugeModel {
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
            _ => Some(GaugeValue::Svg(icon_quantity(
                QuantityStyle::Grid,
                snapshot.strength,
            ))),
        },
        attention,
        on_click: None,
        menu: None,
    }
}

fn wifi_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("wifi", None, move |mut sender| {
        loop {
            let snapshot = wifi_snapshot();
            let _ = sender.try_send(wifi_gauge(snapshot));
            thread::sleep(POLL_INTERVAL);
        }
    })
}

pub fn wifi_subscription() -> Subscription<Message> {
    Subscription::run(|| wifi_stream().map(Message::Gauge))
}
