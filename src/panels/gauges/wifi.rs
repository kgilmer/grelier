// Wi-Fi signal/connection gauge that polls sysfs and /proc.
// Consumes Settings: grelier.gauge.wifi.*.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{
    GaugeDisplay, GaugeMenu, GaugeMenuItem, GaugeModel, GaugeValue, GaugeValueAttention,
    MenuSelectAction, event_stream,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings;
use crate::settings::SettingSpec;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const SYS_NET: &str = "/sys/class/net";
const PROC_NET_WIRELESS: &str = "/proc/net/wireless";
const WPA_CTRL_DIRS: [&str; 2] = ["/run/wpa_supplicant", "/var/run/wpa_supplicant"];
const DEFAULT_QUALITY_MAX: f32 = 70.0;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 3;
const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const NM_SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const NM_SETTINGS_CONNECTION_IFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const NM_DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const NM_DEVICE_WIRELESS_IFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const NM_ACCESS_POINT_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";

#[derive(Clone, Copy, Debug)]
enum WifiState {
    Connected,
    NotConnected,
    NoDevice,
}

#[derive(Clone, Debug)]
struct WifiSnapshot {
    state: WifiState,
    iface: Option<String>,
    ssid: Option<String>,
    strength: f32,
}

#[derive(Clone, Debug)]
struct WifiMenuEntry {
    id: String,
    path: OwnedObjectPath,
    ssid: Option<String>,
}

#[derive(Debug)]
enum WifiCommand {
    Connect(String),
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

fn read_ssid(iface: &str) -> Option<String> {
    for dir in WPA_CTRL_DIRS {
        let path = Path::new(dir).join(iface);
        if !path.exists() {
            continue;
        }
        if let Some(ssid) = read_ssid_wpa_ctrl(&path) {
            return Some(ssid);
        }
    }
    read_ssid_network_manager(iface)
}

fn read_ssid_network_manager(iface: &str) -> Option<String> {
    let connection = Connection::system().ok()?;
    let nm_proxy = Proxy::new(&connection, NM_SERVICE, NM_PATH, NM_IFACE).ok()?;
    let device_path: OwnedObjectPath = nm_proxy.call("GetDeviceByIpIface", &(iface)).ok()?;
    let wifi_proxy = Proxy::new(
        &connection,
        NM_SERVICE,
        device_path,
        NM_DEVICE_WIRELESS_IFACE,
    )
    .ok()?;
    let ap_path: OwnedObjectPath = wifi_proxy.get_property("ActiveAccessPoint").ok()?;
    if ap_path.as_str() == "/" {
        return None;
    }
    let ap_proxy = Proxy::new(&connection, NM_SERVICE, ap_path, NM_ACCESS_POINT_IFACE).ok()?;
    let ssid_bytes: Vec<u8> = ap_proxy.get_property("Ssid").ok()?;
    normalize_ssid_bytes(&ssid_bytes)
}

fn read_ssid_wpa_ctrl(path: &Path) -> Option<String> {
    let temp_path = temp_socket_path()?;
    let _temp_guard = TempSocketGuard::new(&temp_path);
    let socket = UnixDatagram::bind(&temp_path).ok()?;
    socket
        .set_read_timeout(Some(Duration::from_millis(250)))
        .ok()?;
    socket.send_to(b"STATUS", path).ok()?;
    let mut buf = [0u8; 4096];
    let size = socket.recv(&mut buf).ok()?;
    let response = String::from_utf8_lossy(&buf[..size]);
    for line in response.lines() {
        if let Some(rest) = line.strip_prefix("ssid=") {
            return normalize_ssid(rest);
        }
    }
    None
}

fn temp_socket_path() -> Option<PathBuf> {
    let mut path = std::env::temp_dir();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    path.push(format!(
        "grelier_wpa_ctrl_{}_{}.sock",
        std::process::id(),
        now
    ));
    Some(path)
}

struct TempSocketGuard {
    path: PathBuf,
}

impl TempSocketGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl Drop for TempSocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn normalize_ssid(raw: &str) -> Option<String> {
    let ssid = raw.trim();
    if ssid.is_empty() || ssid.eq_ignore_ascii_case("off/any") {
        None
    } else {
        Some(ssid.to_string())
    }
}

fn normalize_ssid_bytes(raw: &[u8]) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let ssid = String::from_utf8_lossy(raw);
    normalize_ssid(ssid.trim_matches('\0'))
}

fn nm_device_path(connection: &Connection, iface: &str) -> Option<OwnedObjectPath> {
    let nm_proxy = Proxy::new(connection, NM_SERVICE, NM_PATH, NM_IFACE).ok()?;
    nm_proxy.call("GetDeviceByIpIface", &(iface)).ok()
}

fn active_connection_path(
    connection: &Connection,
    device_path: &OwnedObjectPath,
) -> Option<OwnedObjectPath> {
    let device_proxy = Proxy::new(connection, NM_SERVICE, device_path, NM_DEVICE_IFACE).ok()?;
    let active_path: OwnedObjectPath = device_proxy.get_property("ActiveConnection").ok()?;
    if active_path.as_str() == "/" {
        None
    } else {
        Some(active_path)
    }
}

fn active_settings_connection_path(
    connection: &Connection,
    active_path: &OwnedObjectPath,
) -> Option<OwnedObjectPath> {
    let active_proxy = Proxy::new(
        connection,
        NM_SERVICE,
        active_path,
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .ok()?;
    let settings_path: OwnedObjectPath = active_proxy.get_property("Connection").ok()?;
    if settings_path.as_str() == "/" {
        None
    } else {
        Some(settings_path)
    }
}

fn wifi_connection_entries(
    connection: &Connection,
    available_ssids: Option<&HashSet<String>>,
) -> Vec<WifiMenuEntry> {
    let settings_proxy =
        match Proxy::new(connection, NM_SERVICE, NM_SETTINGS_PATH, NM_SETTINGS_IFACE) {
            Ok(proxy) => proxy,
            Err(_) => return Vec::new(),
        };
    let paths: Vec<OwnedObjectPath> = match settings_proxy.call("ListConnections", &()) {
        Ok(paths) => paths,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for path in paths {
        let conn_proxy = match Proxy::new(
            connection,
            NM_SERVICE,
            path.clone(),
            NM_SETTINGS_CONNECTION_IFACE,
        ) {
            Ok(proxy) => proxy,
            Err(_) => continue,
        };
        let settings: HashMap<String, HashMap<String, OwnedValue>> =
            match conn_proxy.call("GetSettings", &()) {
                Ok(settings) => settings,
                Err(_) => continue,
            };
        let Some(connection_section) = settings.get("connection") else {
            continue;
        };
        let connection_type: Option<String> = connection_section
            .get("type")
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| value.try_into().ok());
        if connection_type.as_deref() != Some("802-11-wireless") {
            continue;
        }
        let id = connection_section
            .get("id")
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| value.try_into().ok())
            .unwrap_or_else(|| path.as_str().to_string());
        let timestamp: Option<u64> = connection_section
            .get("timestamp")
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| value.try_into().ok());
        if let Some(last_seen) = timestamp
            && last_seen == 0
        {
            continue;
        }
        let ssid = settings
            .get("802-11-wireless")
            .and_then(|section| section.get("ssid"))
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| value.try_into().ok())
            .and_then(|bytes: Vec<u8>| normalize_ssid_bytes(&bytes));
        if let Some(available) = available_ssids {
            if let Some(ssid) = ssid.as_deref() {
                if !available.contains(ssid) {
                    continue;
                }
            } else {
                continue;
            }
        }
        entries.push(WifiMenuEntry { id, path, ssid });
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

fn connection_label(entry: &WifiMenuEntry) -> String {
    if let Some(ssid) = entry.ssid.as_ref()
        && !ssid.trim().is_empty()
    {
        return ssid.clone();
    }
    if !entry.id.trim().is_empty() {
        return entry.id.clone();
    }
    entry
        .ssid
        .clone()
        .unwrap_or_else(|| "Unknown network".to_string())
}

fn wifi_menu_items(
    entries: &[WifiMenuEntry],
    active_connection: Option<&OwnedObjectPath>,
    active_ssid: Option<&str>,
) -> Vec<GaugeMenuItem> {
    let mut items: Vec<GaugeMenuItem> = entries
        .iter()
        .map(|entry| {
            let selected = active_connection.is_some_and(|path| path == &entry.path)
                || (active_connection.is_none()
                    && active_ssid.is_some_and(|ssid| entry.ssid.as_deref() == Some(ssid)));
            let label = connection_label(entry);
            GaugeMenuItem {
                id: entry.path.as_str().to_string(),
                label,
                selected,
            }
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn activate_connection(
    connection: &Connection,
    connection_path: &str,
    device_path: &OwnedObjectPath,
) -> bool {
    let nm_proxy = match Proxy::new(connection, NM_SERVICE, NM_PATH, NM_IFACE) {
        Ok(proxy) => proxy,
        Err(_) => return false,
    };
    let connection_path = match OwnedObjectPath::try_from(connection_path) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let specific = match OwnedObjectPath::try_from("/") {
        Ok(path) => path,
        Err(_) => return false,
    };
    nm_proxy
        .call::<_, _, OwnedObjectPath>(
            "ActivateConnection",
            &(connection_path, device_path, specific),
        )
        .is_ok()
}

fn available_ssids(connection: &Connection, device_path: &OwnedObjectPath) -> HashSet<String> {
    let mut ssids = HashSet::new();
    let device_proxy = match Proxy::new(
        connection,
        NM_SERVICE,
        device_path,
        NM_DEVICE_WIRELESS_IFACE,
    ) {
        Ok(proxy) => proxy,
        Err(_) => return ssids,
    };
    let ap_paths: Vec<OwnedObjectPath> = match device_proxy.call("GetAllAccessPoints", &()) {
        Ok(paths) => paths,
        Err(_) => return ssids,
    };
    for ap_path in ap_paths {
        let ap_proxy = match Proxy::new(connection, NM_SERVICE, ap_path, NM_ACCESS_POINT_IFACE) {
            Ok(proxy) => proxy,
            Err(_) => continue,
        };
        let ssid_bytes: Vec<u8> = match ap_proxy.get_property("Ssid") {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if let Some(ssid) = normalize_ssid_bytes(&ssid_bytes) {
            ssids.insert(ssid);
        }
    }
    ssids
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
            iface: None,
            ssid: None,
            strength: 0.0,
        };
    }

    let Some(iface) = pick_interface(&ifaces, sys_net, proc_net_wireless) else {
        return WifiSnapshot {
            state: WifiState::NoDevice,
            iface: None,
            ssid: None,
            strength: 0.0,
        };
    };

    let path = PathBuf::from(sys_net).join(&iface);
    let quality = read_link_quality_at(proc_net_wireless, &iface);
    let connected = interface_connected(&path, quality);
    let strength = quality.unwrap_or(0.0).clamp(0.0, quality_max) / quality_max;
    let ssid = if connected { read_ssid(&iface) } else { None };

    WifiSnapshot {
        state: if connected {
            WifiState::Connected
        } else {
            WifiState::NotConnected
        },
        iface: Some(iface),
        ssid,
        strength,
    }
}

fn wifi_info_dialog(snapshot: &WifiSnapshot) -> InfoDialog {
    let device_line = snapshot
        .iface
        .clone()
        .unwrap_or_else(|| "No wireless device".to_string());
    let ssid_line = match snapshot.state {
        WifiState::Connected => snapshot
            .ssid
            .clone()
            .unwrap_or_else(|| "Unknown SSID".to_string()),
        WifiState::NotConnected => "Not connected".to_string(),
        WifiState::NoDevice => "No device".to_string(),
    };
    let signal_line = format!("Signal: {:.0}%", snapshot.strength * 100.0);

    InfoDialog {
        title: "Wi-Fi".to_string(),
        lines: vec![device_line, ssid_line, signal_line],
    }
}

fn wifi_gauge(snapshot: WifiSnapshot, menu: Option<GaugeMenu>) -> GaugeModel {
    let (icon, attention) = match snapshot.state {
        WifiState::Connected => ("wifi.svg", GaugeValueAttention::Nominal),
        WifiState::NotConnected => ("wifi-off.svg", GaugeValueAttention::Warning),
        WifiState::NoDevice => ("wifi-no.svg", GaugeValueAttention::Danger),
    };

    GaugeModel {
        id: "wifi",
        icon: Some(svg_asset(icon)),
        display: match snapshot.state {
            WifiState::NoDevice => GaugeDisplay::Error,
            _ => GaugeDisplay::Value {
                value: GaugeValue::Svg(icon_quantity(snapshot.strength)),
                attention,
            },
        },
        hide_value: false,
        nominal_color: None,
        on_click: None,
        menu,
        action_dialog: None,
        info: Some(wifi_info_dialog(&snapshot)),
    }
}

fn wifi_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    let (command_tx, command_rx) = mpsc::channel::<WifiCommand>();
    let menu_select: MenuSelectAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |connection_path: String| {
            let _ = command_tx.send(WifiCommand::Connect(connection_path));
        })
    };

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
            let nm_connection = Connection::system().ok();

            while let Ok(command) = command_rx.try_recv() {
                if let Some(connection) = nm_connection.as_ref() {
                    let WifiCommand::Connect(connection_path) = command;
                    if let Some(iface) = snapshot.iface.as_deref()
                        && let Some(device_path) = nm_device_path(connection, iface)
                    {
                        let _ = activate_connection(connection, &connection_path, &device_path);
                    }
                }
            }

            let menu = if let Some(connection) = nm_connection.as_ref()
                && let Some(iface) = snapshot.iface.as_deref()
                && let Some(device_path) = nm_device_path(connection, iface)
            {
                let available = available_ssids(connection, &device_path);
                let entries = wifi_connection_entries(connection, Some(&available));
                let active_connection = active_connection_path(connection, &device_path)
                    .and_then(|path| active_settings_connection_path(connection, &path));
                let items = wifi_menu_items(
                    &entries,
                    active_connection.as_ref(),
                    snapshot.ssid.as_deref(),
                );
                Some(GaugeMenu {
                    title: "Wi-Fi Networks".to_string(),
                    items,
                    on_select: Some(menu_select.clone()),
                })
            } else {
                None
            };

            let _ = sender.try_send(wifi_gauge(snapshot, menu));
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
