// Wi-Fi signal/connection gauge that polls sysfs and /proc.
// Consumes Settings: grelier.gauge.wifi.*.
use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention, SettingSpec, event_stream};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::icon::{icon_quantity, svg_asset};
use crate::settings;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const SYS_NET: &str = "/sys/class/net";
const PROC_NET_WIRELESS: &str = "/proc/net/wireless";
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
    wifi_interfaces_at(Path::new(SYS_NET))
}

fn wifi_interfaces_at(sys_net: &Path) -> Vec<String> {
    let mut ifaces = Vec::new();
    let entries = match fs::read_dir(sys_net) {
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
    read_link_quality_at(Path::new(PROC_NET_WIRELESS), iface)
}

fn read_link_quality_at(proc_net_wireless: &Path, iface: &str) -> Option<f32> {
    let contents = fs::read_to_string(proc_net_wireless).ok()?;
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

fn pick_interface(ifaces: &[String], sys_net: &Path, proc_net_wireless: &Path) -> Option<String> {
    for iface in ifaces {
        let path = PathBuf::from(sys_net).join(iface);
        let quality = read_link_quality_at(proc_net_wireless, iface);
        if interface_connected(&path, quality) {
            return Some(iface.clone());
        }
    }

    ifaces.first().cloned()
}

fn wifi_snapshot(quality_max: f32) -> WifiSnapshot {
    wifi_snapshot_with_paths(
        Path::new(SYS_NET),
        Path::new(PROC_NET_WIRELESS),
        quality_max,
    )
}

fn wifi_snapshot_with_paths(
    sys_net: &Path,
    proc_net_wireless: &Path,
    quality_max: f32,
) -> WifiSnapshot {
    let ifaces = wifi_interfaces_at(sys_net);
    if ifaces.is_empty() {
        return WifiSnapshot {
            state: WifiState::NoDevice,
            strength: 0.0,
        };
    }

    let Some(iface) = pick_interface(&ifaces, sys_net, proc_net_wireless) else {
        return WifiSnapshot {
            state: WifiState::NoDevice,
            strength: 0.0,
        };
    };

    let path = PathBuf::from(sys_net).join(&iface);
    let quality = read_link_quality_at(proc_net_wireless, &iface);
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
            _ => Some(GaugeValue::Svg(icon_quantity(snapshot.strength))),
        },
        attention,
        on_click: None,
        menu: None,
        info: None,
    }
}

fn wifi_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("wifi", None, move |mut sender| {
        let mut quality_max = settings::settings()
            .get_parsed_or("grelier.gauge.wifi.quality_max", DEFAULT_QUALITY_MAX);
        if quality_max <= 0.0 {
            quality_max = DEFAULT_QUALITY_MAX;
        }
        let poll_interval_secs = settings::settings().get_parsed_or(
            "grelier.gauge.wifi.poll_interval_secs",
            DEFAULT_POLL_INTERVAL_SECS,
        );
        let poll_interval = Duration::from_secs(poll_interval_secs);
        loop {
            let snapshot = wifi_snapshot(quality_max);
            let _ = sender.try_send(wifi_gauge(snapshot));
            thread::sleep(poll_interval);
        }
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.wifi.quality_max",
            default: "70",
        },
        SettingSpec {
            key: "grelier.gauge.wifi.poll_interval_secs",
            default: "3",
        },
    ];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(wifi_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "wifi",
        label: "Wi-Fi",
        description: "Wi-Fi signal gauge showing percent link quality and current SSID.",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        dir.push(format!(
            "grelier_wifi_test_{}_{}_{}",
            name,
            std::process::id(),
            id
        ));
        dir
    }

    fn write_iface(sys_net: &Path, iface: &str, carrier: Option<&str>, operstate: Option<&str>) {
        let iface_dir = sys_net.join(iface);
        fs::create_dir_all(iface_dir.join("wireless")).expect("create wireless dir");
        if let Some(carrier) = carrier {
            fs::write(iface_dir.join("carrier"), carrier).expect("write carrier");
        }
        if let Some(operstate) = operstate {
            fs::write(iface_dir.join("operstate"), operstate).expect("write operstate");
        }
    }

    fn write_wireless(proc_path: &Path, lines: &[&str]) {
        let mut contents = String::new();
        contents.push_str(
            "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE\n",
        );
        contents.push_str(
            " face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22\n",
        );
        for line in lines {
            contents.push_str(line);
            contents.push('\n');
        }
        fs::write(proc_path, contents).expect("write wireless file");
    }

    #[test]
    fn pick_interface_prefers_connected() {
        let dir = temp_dir("pick");
        let sys_net = dir.join("sys_net");
        fs::create_dir_all(&sys_net).expect("create sys_net");
        let proc_wireless = dir.join("wireless");

        write_iface(&sys_net, "wlan0", Some("0"), Some("down"));
        write_iface(&sys_net, "wlan1", Some("1"), Some("up"));
        write_wireless(
            &proc_wireless,
            &[
                "wlan0: 0000 10. 0. 0. 0. 0. 0.",
                "wlan1: 0000 50. 0. 0. 0. 0. 0.",
            ],
        );

        let ifaces = vec!["wlan0".to_string(), "wlan1".to_string()];
        let picked = pick_interface(&ifaces, &sys_net, &proc_wireless).expect("pick iface");
        assert_eq!(picked, "wlan1");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_clamps_strength_and_marks_states() {
        let dir = temp_dir("snapshot");
        let sys_net = dir.join("sys_net");
        fs::create_dir_all(&sys_net).expect("create sys_net");
        let proc_wireless = dir.join("wireless");

        write_iface(&sys_net, "wlan0", Some("1"), Some("up"));
        write_wireless(&proc_wireless, &["wlan0: 0000 100. 0. 0. 0. 0. 0."]);

        let snapshot = wifi_snapshot_with_paths(&sys_net, &proc_wireless, 70.0);
        assert!(matches!(snapshot.state, WifiState::Connected));
        assert!((snapshot.strength - 1.0).abs() < f32::EPSILON);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_reports_no_device() {
        let dir = temp_dir("no_device");
        let sys_net = dir.join("sys_net");
        fs::create_dir_all(&sys_net).expect("create sys_net");
        let proc_wireless = dir.join("wireless");
        write_wireless(&proc_wireless, &[]);

        let snapshot = wifi_snapshot_with_paths(&sys_net, &proc_wireless, 70.0);
        assert!(matches!(snapshot.state, WifiState::NoDevice));
        assert_eq!(snapshot.strength, 0.0);

        let _ = fs::remove_dir_all(dir);
    }
}
