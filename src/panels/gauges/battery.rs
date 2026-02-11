// Battery gauge driven by udev power_supply events and snapshots.
use crate::icon::{icon_quantity, svg_asset};
use crate::info_dialog::InfoDialog;
use crate::panels::gauges::gauge::{
    GaugeMenu, GaugeMenuItem, GaugeModel, GaugeNominalColor, GaugeValue, GaugeValueAttention,
    MenuSelectAction, event_stream,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings;
use crate::settings::SettingSpec;
use battery::State as BatteryState;
use battery::units::{energy::watt_hour, time::second};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

const DEFAULT_WARNING_PERCENT: u8 = 49;
const DEFAULT_DANGER_PERCENT: u8 = 19;
const VALUE_ICON_SUCCESS_THRESHOLD: u8 = 50;
const VALUE_ICON_WARNING_THRESHOLD: u8 = 10;
const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 5;
const PPD_SERVICE: &str = "net.hadess.PowerProfiles";
const PPD_PATH: &str = "/net/hadess/PowerProfiles";
const PPD_IFACE: &str = "net.hadess.PowerProfiles";
const SYS_PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";
const SYS_PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";

#[derive(Debug)]
enum BatteryCommand {
    SetPowerProfile(String),
}

#[derive(Debug)]
struct PowerProfilesSnapshot {
    profiles: Vec<String>,
    active: String,
}

/// Stream battery information via udev power_supply events.
fn battery_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    let (command_tx, command_rx) = mpsc::channel::<BatteryCommand>();
    let menu_select: MenuSelectAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |profile: String| {
            let _ = command_tx.send(BatteryCommand::SetPowerProfile(profile));
        })
    };

    thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            match command {
                BatteryCommand::SetPowerProfile(profile) => {
                    if !set_active_power_profile(&profile) {
                        log::error!("battery gauge: failed to set power profile to '{profile}'");
                    }
                }
            }
        }
    });

    event_stream("battery", None, move |mut sender| {
        let warning_percent = settings::settings().get_parsed_or(
            "grelier.gauge.battery.warning_percent",
            DEFAULT_WARNING_PERCENT,
        );
        let danger_percent = settings::settings().get_parsed_or(
            "grelier.gauge.battery.danger_percent",
            DEFAULT_DANGER_PERCENT,
        );
        let manager = battery::Manager::new().ok();
        let refresh_interval_secs = settings::settings().get_parsed_or(
            "grelier.gauge.battery.refresh_interval_secs",
            DEFAULT_REFRESH_INTERVAL_SECS,
        );
        let info_state = Arc::new(Mutex::new(InfoDialog {
            title: "Battery".to_string(),
            lines: vec![
                "Total: Unknown".to_string(),
                "Current: Unknown".to_string(),
                "ETA: Unknown".to_string(),
                "Discharge rate: Unknown".to_string(),
            ],
        }));
        // Send current state so the UI shows something before the first event.
        send_snapshot(
            &mut sender,
            warning_percent,
            danger_percent,
            &info_state,
            manager.as_ref(),
            Some(menu_select.clone()),
        );

        // Try to open a udev monitor; if it fails, just exit the worker.
        let monitor = match udev::MonitorBuilder::new()
            .and_then(|m| m.match_subsystem("power_supply"))
            .and_then(|m| m.listen())
        {
            Ok(m) => m,
            Err(err) => {
                log::error!("battery gauge: failed to start udev monitor: {err}");
                return;
            }
        };

        let mut poll_sender = sender.clone();
        let poll_info_state = Arc::clone(&info_state);
        let poll_menu_select = menu_select.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(refresh_interval_secs));
                if let Some(model) = snapshot_model(
                    warning_percent,
                    danger_percent,
                    &poll_info_state,
                    None,
                    Some(&poll_menu_select),
                ) {
                    let _ = poll_sender.try_send(model);
                }
            }
        });

        for event in monitor.iter() {
            let dev = event.device();
            if is_mains(&dev) {
                let online = mains_online(&dev);
                let status = property_str(&dev, "POWER_SUPPLY_STATUS");
                log::error!(
                    "battery gauge: power_supply event mains online={online:?} status={status:?}"
                );
            } else if is_battery(&dev) {
                let status = property_str(&dev, "POWER_SUPPLY_STATUS");
                let capacity = property_str(&dev, "POWER_SUPPLY_CAPACITY")
                    .or_else(|| property_str(&dev, "CAPACITY"));
                log::error!(
                    "battery gauge: power_supply event battery status={status:?} capacity={capacity:?}"
                );
            }
            if let Some(model) = snapshot_model(
                warning_percent,
                danger_percent,
                &info_state,
                manager.as_ref(),
                Some(&menu_select),
            ) {
                let _ = sender.try_send(model);
            }
        }
    })
}

fn send_snapshot(
    sender: &mut iced::futures::channel::mpsc::Sender<GaugeModel>,
    warning_percent: u8,
    danger_percent: u8,
    info_state: &Arc<Mutex<InfoDialog>>,
    manager: Option<&battery::Manager>,
    menu_select: Option<MenuSelectAction>,
) {
    if let Some(model) = snapshot_model(
        warning_percent,
        danger_percent,
        info_state,
        manager,
        menu_select.as_ref(),
    ) {
        let _ = sender.try_send(model);
    }
}

fn is_battery(dev: &udev::Device) -> bool {
    dev.property_value("POWER_SUPPLY_TYPE")
        .and_then(|v| v.to_str())
        .map(|v| v.eq_ignore_ascii_case("Battery"))
        .unwrap_or(false)
}

fn is_mains(dev: &udev::Device) -> bool {
    dev.property_value("POWER_SUPPLY_TYPE")
        .and_then(|v| v.to_str())
        .map(|v| v.eq_ignore_ascii_case("Mains"))
        .unwrap_or(false)
}

fn mains_online(dev: &udev::Device) -> Option<bool> {
    property_str(dev, "POWER_SUPPLY_ONLINE")
        .or_else(|| property_str(dev, "ONLINE"))
        .and_then(|value| match value.trim() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        })
}

fn snapshot_model(
    warning_percent: u8,
    danger_percent: u8,
    info_state: &Arc<Mutex<InfoDialog>>,
    manager: Option<&battery::Manager>,
    menu_select: Option<&MenuSelectAction>,
) -> Option<GaugeModel> {
    let mut enumerator = match udev::Enumerator::new() {
        Ok(e) => e,
        Err(err) => {
            log::error!("battery gauge: failed to enumerate devices: {err}");
            return None;
        }
    };

    if enumerator.match_subsystem("power_supply").is_err() {
        log::error!("battery gauge: failed to set subsystem filter");
        return None;
    }

    let devices = match enumerator.scan_devices() {
        Ok(list) => list,
        Err(err) => {
            log::error!("battery gauge: failed to scan devices: {err}");
            return None;
        }
    };

    let mut battery_dev: Option<udev::Device> = None;
    let mut ac_online: Option<bool> = None;

    for dev in devices {
        if battery_dev.is_none() && is_battery(&dev) {
            battery_dev = Some(dev);
            continue;
        }
        if ac_online.is_none() && is_mains(&dev) {
            ac_online = mains_online(&dev);
        }
        if battery_dev.is_some() && ac_online.is_some() {
            break;
        }
    }

    if let Some(dev) = battery_dev {
        update_info_state(info_state, battery_info_dialog(&dev, manager));
        let status = property_str(&dev, "POWER_SUPPLY_STATUS");
        if ac_online.is_none() {
            ac_online = ac_online_from_status(status.as_deref());
        }
        if let Some((value, attention)) = battery_value(&dev, warning_percent, danger_percent) {
            let capacity = property_str(&dev, "POWER_SUPPLY_CAPACITY")
                .or_else(|| property_str(&dev, "CAPACITY"));
            let capacity_percent = capacity.as_deref().and_then(|cap| cap.parse::<u8>().ok());
            let status_full = status
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("Full"))
                .unwrap_or(false);
            let is_full = status_full || capacity_percent.map(|value| value >= 95).unwrap_or(false);
            let nominal_color = if matches!(ac_online, Some(true)) && is_full {
                Some(GaugeNominalColor::Primary)
            } else {
                None
            };
            let icon = Some(svg_asset(power_icon_for_status(
                status.as_deref(),
                ac_online,
            )));
            let menu = menu_select.and_then(|select| power_profile_menu(select.clone()));
            return Some(GaugeModel {
                id: "battery",
                icon,
                value,
                attention,
                nominal_color,
                on_click: None,
                menu,
                info: info_state.lock().ok().map(|info| info.clone()),
            });
        }
    }

    update_info_state(
        info_state,
        InfoDialog {
            title: "Battery".to_string(),
            lines: vec![
                "Total: Unknown".to_string(),
                "Current: Unknown".to_string(),
                "ETA: Unknown".to_string(),
                "Discharge rate: Unknown".to_string(),
            ],
        },
    );
    let menu = menu_select.and_then(|select| power_profile_menu(select.clone()));
    Some(GaugeModel {
        id: "battery",
        icon: Some(svg_asset("power.svg")),
        value: None,
        attention: GaugeValueAttention::Danger,
        nominal_color: None,
        on_click: None,
        menu,
        info: info_state.lock().ok().map(|info| info.clone()),
    })
}

fn battery_value(
    dev: &udev::Device,
    warning_percent: u8,
    danger_percent: u8,
) -> Option<(Option<GaugeValue>, GaugeValueAttention)> {
    let capacity =
        property_str(dev, "POWER_SUPPLY_CAPACITY").or_else(|| property_str(dev, "CAPACITY"));
    let status = property_str(dev, "POWER_SUPPLY_STATUS");
    battery_value_from_strings(
        capacity.as_deref(),
        status.as_deref(),
        warning_percent,
        danger_percent,
    )
}

fn battery_value_from_strings(
    capacity: Option<&str>,
    status: Option<&str>,
    _warning_percent: u8,
    _danger_percent: u8,
) -> Option<(Option<GaugeValue>, GaugeValueAttention)> {
    if let Some(cap) = capacity {
        if let Ok(percent) = cap.parse::<u8>() {
            let attention = attention_for_capacity(percent);
            return Some((
                Some(GaugeValue::Svg(icon_quantity(percent as f32 / 100.0))),
                attention,
            ));
        }

        if let Some(status) = status {
            return Some((
                Some(GaugeValue::Text(format!("{cap}% ({status})"))),
                GaugeValueAttention::Nominal,
            ));
        }

        return Some((
            Some(GaugeValue::Text(format!("{cap}%"))),
            GaugeValueAttention::Nominal,
        ));
    }

    Some((None, GaugeValueAttention::Danger))
}

fn update_info_state(info_state: &Arc<Mutex<InfoDialog>>, dialog: InfoDialog) {
    if let Ok(mut info) = info_state.lock() {
        *info = dialog;
    }
}

fn battery_info_dialog(dev: &udev::Device, manager: Option<&battery::Manager>) -> InfoDialog {
    let status = property_str(dev, "POWER_SUPPLY_STATUS");
    let rate_line = discharge_rate_line(status.as_deref(), discharge_rate_watts_from_udev(dev));
    if let Some(mut dialog) = manager.and_then(battery_info_dialog_from_manager) {
        dialog.lines.push(rate_line);
        return dialog;
    }
    let mut dialog = battery_info_dialog_from_udev(dev, status.as_deref());
    dialog.lines.push(rate_line);
    dialog
}

fn battery_info_dialog_from_manager(manager: &battery::Manager) -> Option<InfoDialog> {
    let mut batteries = manager.batteries().ok()?;
    let battery = batteries.next()?.ok()?;
    let total = battery.energy_full().get::<watt_hour>() as f64;
    let current = battery.energy().get::<watt_hour>() as f64;
    let eta_seconds = battery
        .time_to_empty()
        .map(|time| time.get::<second>())
        .and_then(|value| {
            if value > 0.0 {
                Some(value.round() as u64)
            } else {
                None
            }
        });
    let eta_line = match eta_seconds {
        Some(seconds) => format!("ETA: {}", format_duration(seconds)),
        None => match battery.state() {
            BatteryState::Charging => "ETA: Charging".to_string(),
            BatteryState::Full => "ETA: Full".to_string(),
            _ => "ETA: Unknown".to_string(),
        },
    };

    Some(InfoDialog {
        title: "Battery".to_string(),
        lines: vec![
            format!("Total: {}", format_quantity(Some(total), Some("Wh"))),
            format!("Current: {}", format_quantity(Some(current), Some("Wh"))),
            eta_line,
        ],
    })
}

fn battery_info_dialog_from_udev(dev: &udev::Device, status: Option<&str>) -> InfoDialog {
    let (total, current, unit) = battery_charge_values(dev);
    let eta_seconds = time_to_empty_seconds(dev)
        .or_else(|| estimate_time_to_empty_seconds_from_udev(dev, status));
    let is_charging = is_charging_status(status);
    let eta_line = match eta_seconds {
        Some(seconds) if seconds > 0 => format!("ETA: {}", format_duration(seconds)),
        _ if is_charging => "ETA: Charging".to_string(),
        _ => "ETA: Unknown".to_string(),
    };

    InfoDialog {
        title: "Battery".to_string(),
        lines: vec![
            format!("Total: {}", format_quantity(total, unit)),
            format!("Current: {}", format_quantity(current, unit)),
            eta_line,
        ],
    }
}

fn battery_charge_values(dev: &udev::Device) -> (Option<f64>, Option<f64>, Option<&'static str>) {
    let energy_full = property_num(dev, "POWER_SUPPLY_ENERGY_FULL")
        .or_else(|| property_num(dev, "ENERGY_FULL"))
        .or_else(|| property_num(dev, "POWER_SUPPLY_ENERGY_FULL_DESIGN"))
        .or_else(|| property_num(dev, "ENERGY_FULL_DESIGN"));
    let energy_now =
        property_num(dev, "POWER_SUPPLY_ENERGY_NOW").or_else(|| property_num(dev, "ENERGY_NOW"));
    if energy_full.is_some() || energy_now.is_some() {
        return (
            energy_full.map(|value| value / 1_000_000.0),
            energy_now.map(|value| value / 1_000_000.0),
            Some("Wh"),
        );
    }

    let charge_full = property_num(dev, "POWER_SUPPLY_CHARGE_FULL")
        .or_else(|| property_num(dev, "CHARGE_FULL"))
        .or_else(|| property_num(dev, "POWER_SUPPLY_CHARGE_FULL_DESIGN"))
        .or_else(|| property_num(dev, "CHARGE_FULL_DESIGN"));
    let charge_now =
        property_num(dev, "POWER_SUPPLY_CHARGE_NOW").or_else(|| property_num(dev, "CHARGE_NOW"));
    if charge_full.is_some() || charge_now.is_some() {
        return (
            charge_full.map(|value| value / 1_000_000.0),
            charge_now.map(|value| value / 1_000_000.0),
            Some("Ah"),
        );
    }

    (None, None, None)
}

fn discharge_rate_watts_from_udev(dev: &udev::Device) -> Option<f64> {
    let power = property_num(dev, "POWER_SUPPLY_POWER_NOW")
        .or_else(|| property_num(dev, "POWER_NOW"))
        .map(|value| value / 1_000_000.0);
    if power.is_some() {
        return power;
    }

    let current = property_num(dev, "POWER_SUPPLY_CURRENT_NOW")
        .or_else(|| property_num(dev, "CURRENT_NOW"))?;
    let voltage = property_num(dev, "POWER_SUPPLY_VOLTAGE_NOW")
        .or_else(|| property_num(dev, "VOLTAGE_NOW"))?;
    Some((current / 1_000_000.0) * (voltage / 1_000_000.0))
}

fn discharge_rate_line(status: Option<&str>, rate_watts: Option<f64>) -> String {
    if status
        .map(|value| value.eq_ignore_ascii_case("Charging"))
        .unwrap_or(false)
    {
        return "Discharge rate: Charging".to_string();
    }
    if status
        .map(|value| value.eq_ignore_ascii_case("Full"))
        .unwrap_or(false)
    {
        return "Discharge rate: Full".to_string();
    }
    match rate_watts {
        Some(rate) => format!(
            "Discharge rate: {}",
            format_quantity(Some(rate.abs()), Some("W"))
        ),
        None => "Discharge rate: Unknown".to_string(),
    }
}

fn time_to_empty_seconds(dev: &udev::Device) -> Option<u64> {
    property_num(dev, "POWER_SUPPLY_TIME_TO_EMPTY_NOW")
        .or_else(|| property_num(dev, "TIME_TO_EMPTY_NOW"))
        .or_else(|| property_num(dev, "POWER_SUPPLY_TIME_TO_EMPTY_AVG"))
        .or_else(|| property_num(dev, "TIME_TO_EMPTY_AVG"))
        .and_then(|value| {
            if value <= 0.0 {
                None
            } else {
                Some(value.round() as u64)
            }
        })
}

fn estimate_time_to_empty_seconds_from_udev(
    dev: &udev::Device,
    status: Option<&str>,
) -> Option<u64> {
    let (_, current, unit) = battery_charge_values(dev);
    let current_wh = match (current, unit) {
        (Some(value), Some("Wh")) => Some(value),
        _ => None,
    };
    let rate_watts = discharge_rate_watts_from_udev(dev).map(|value| value.abs());
    estimate_time_to_empty_seconds(status, current_wh, rate_watts)
}

fn estimate_time_to_empty_seconds(
    status: Option<&str>,
    current_wh: Option<f64>,
    rate_watts: Option<f64>,
) -> Option<u64> {
    if status.map(|value| value.eq_ignore_ascii_case("Discharging")) != Some(true) {
        return None;
    }
    let current_wh = current_wh?;
    let rate_watts = rate_watts?;
    if rate_watts <= 0.0 {
        return None;
    }
    let hours = current_wh / rate_watts;
    if hours <= 0.0 {
        return None;
    }
    Some((hours * 3600.0).round() as u64)
}

fn is_charging_status(status: Option<&str>) -> bool {
    status
        .map(|value| value.eq_ignore_ascii_case("Charging") || value.eq_ignore_ascii_case("Full"))
        .unwrap_or(false)
}

fn format_quantity(value: Option<f64>, unit: Option<&'static str>) -> String {
    match value {
        Some(value) => {
            let formatted = format_number(value);
            match unit {
                Some(unit) => format!("{formatted} {unit}"),
                None => formatted,
            }
        }
        None => "Unknown".to_string(),
    }
}

fn format_number(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn property_num(dev: &udev::Device, key: &str) -> Option<f64> {
    property_str(dev, key).and_then(|value| value.parse::<f64>().ok())
}

fn power_icon_for_status(status: Option<&str>, ac_online: Option<bool>) -> &'static str {
    match status {
        Some(value) if value.eq_ignore_ascii_case("Discharging") => "power-battery-discharge.svg",
        Some(value)
            if value.eq_ignore_ascii_case("Charging")
                || value.eq_ignore_ascii_case("Full")
                || value.eq_ignore_ascii_case("Not charging") =>
        {
            "power-battery-charge.svg"
        }
        _ => match ac_online {
            Some(true) => "power-ac.svg",
            Some(false) => "power-battery-discharge.svg",
            None => "power.svg",
        },
    }
}

fn ac_online_from_status(status: Option<&str>) -> Option<bool> {
    match status {
        Some(value) if value.eq_ignore_ascii_case("Discharging") => Some(false),
        Some(value)
            if value.eq_ignore_ascii_case("Charging")
                || value.eq_ignore_ascii_case("Full")
                || value.eq_ignore_ascii_case("Not charging") =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn attention_for_capacity(percent: u8) -> GaugeValueAttention {
    if percent > VALUE_ICON_SUCCESS_THRESHOLD {
        GaugeValueAttention::Nominal
    } else if percent > VALUE_ICON_WARNING_THRESHOLD {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Danger
    }
}

fn property_str(dev: &udev::Device, key: &str) -> Option<String> {
    dev.property_value(key)
        .and_then(|v| v.to_str())
        .map(|s| s.to_string())
        .or_else(|| {
            dev.attribute_value(key)
                .and_then(|v| v.to_str())
                .map(|s| s.to_string())
        })
}

fn power_profile_menu(on_select: MenuSelectAction) -> Option<GaugeMenu> {
    let snapshot = power_profiles_snapshot()?;
    if snapshot.profiles.is_empty() {
        return None;
    }
    let mut items: Vec<GaugeMenuItem> = snapshot
        .profiles
        .iter()
        .map(|profile| GaugeMenuItem {
            id: profile.clone(),
            label: power_profile_label(profile),
            selected: profile == &snapshot.active,
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    Some(GaugeMenu {
        title: "Power Mode".to_string(),
        items,
        on_select: Some(on_select),
    })
}

fn power_profiles_snapshot() -> Option<PowerProfilesSnapshot> {
    if let Some(snapshot) = power_profiles_snapshot_ppd() {
        return Some(snapshot);
    }
    power_profiles_snapshot_platform()
}

fn power_profiles_snapshot_ppd() -> Option<PowerProfilesSnapshot> {
    let connection = Connection::system().ok()?;
    let proxy = Proxy::new(&connection, PPD_SERVICE, PPD_PATH, PPD_IFACE).ok()?;
    let active: String = proxy.get_property("ActiveProfile").ok()?;
    let profiles: Vec<HashMap<String, OwnedValue>> = proxy.get_property("Profiles").ok()?;
    let mut supported = HashSet::new();
    for entry in profiles {
        if let Some(profile) = power_profile_id(&entry) {
            supported.insert(profile);
        }
    }
    let mut profiles: Vec<String> = supported.into_iter().collect();
    profiles.sort();
    Some(PowerProfilesSnapshot { profiles, active })
}

fn power_profiles_snapshot_platform() -> Option<PowerProfilesSnapshot> {
    let active = fs::read_to_string(SYS_PLATFORM_PROFILE).ok()?;
    let active = active.trim().to_string();
    if active.is_empty() {
        return None;
    }
    let choices = fs::read_to_string(SYS_PLATFORM_PROFILE_CHOICES).ok()?;
    let profiles: Vec<String> = choices
        .split_whitespace()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if profiles.is_empty() {
        return None;
    }
    Some(PowerProfilesSnapshot { profiles, active })
}

fn set_active_power_profile(profile: &str) -> bool {
    if set_active_power_profile_ppd(profile) {
        return true;
    }
    set_active_power_profile_platform(profile)
}

fn set_active_power_profile_ppd(profile: &str) -> bool {
    let connection = match Connection::system() {
        Ok(connection) => connection,
        Err(err) => {
            log::error!("battery gauge: power profiles daemon connection error: {err}");
            return false;
        }
    };
    let proxy = match Proxy::new(&connection, PPD_SERVICE, PPD_PATH, PPD_IFACE) {
        Ok(proxy) => proxy,
        Err(err) => {
            log::error!("battery gauge: power profiles daemon proxy error: {err}");
            return false;
        }
    };
    let profiles: Vec<HashMap<String, OwnedValue>> = match proxy.get_property("Profiles") {
        Ok(profiles) => profiles,
        Err(err) => {
            log::error!("battery gauge: power profiles daemon profiles error: {err}");
            return false;
        }
    };
    let supported = profiles
        .iter()
        .filter_map(power_profile_id)
        .any(|id| id == profile);
    if !supported {
        log::error!("battery gauge: power profiles daemon does not support '{profile}'");
        return false;
    }
    match proxy.set_property("ActiveProfile", profile) {
        Ok(()) => true,
        Err(err) => {
            log::error!("battery gauge: power profiles daemon failed to set '{profile}': {err}");
            false
        }
    }
}

fn set_active_power_profile_platform(profile: &str) -> bool {
    let choices = match fs::read_to_string(SYS_PLATFORM_PROFILE_CHOICES) {
        Ok(choices) => choices,
        Err(err) => {
            log::error!("battery gauge: platform profile choices read error: {err}");
            return false;
        }
    };
    let supported = choices
        .split_whitespace()
        .any(|value| value.trim() == profile);
    if !supported {
        log::error!(
            "battery gauge: platform profile does not support '{profile}' (choices: {choices})"
        );
        return false;
    }
    match fs::write(SYS_PLATFORM_PROFILE, profile) {
        Ok(()) => true,
        Err(err) => {
            log::error!("battery gauge: platform profile failed to set '{profile}': {err}");
            false
        }
    }
}

fn power_profile_id(entry: &HashMap<String, OwnedValue>) -> Option<String> {
    entry
        .get("Profile")
        .or_else(|| entry.get("profile"))
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| value.try_into().ok())
}

fn power_profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => "Power Saver".to_string(),
        "balanced" => "Balanced".to_string(),
        "performance" => "Performance".to_string(),
        other => title_case_profile(other),
    }
}

fn title_case_profile(profile: &str) -> String {
    let mut out = String::new();
    for (idx, word) in profile.split('-').enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.battery.warning_percent",
            default: "49",
        },
        SettingSpec {
            key: "grelier.gauge.battery.danger_percent",
            default: "19",
        },
        SettingSpec {
            key: "grelier.gauge.battery.refresh_interval_secs",
            default: "5",
        },
    ];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(battery_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "battery",
        description: "Battery gauge reporting percent charge and charging status.",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_icon_tracks_state() {
        assert_eq!(
            power_icon_for_status(Some("Charging"), Some(true)),
            "power-battery-charge.svg"
        );
        assert_eq!(
            power_icon_for_status(Some("Discharging"), Some(false)),
            "power-battery-discharge.svg"
        );
        assert_eq!(
            power_icon_for_status(Some("Full"), Some(true)),
            "power-battery-charge.svg"
        );
        assert_eq!(power_icon_for_status(None, Some(true)), "power-ac.svg");
        assert_eq!(
            power_icon_for_status(None, Some(false)),
            "power-battery-discharge.svg"
        );
        assert_eq!(power_icon_for_status(None, None), "power.svg");
    }

    #[test]
    fn ac_online_tracks_status() {
        assert_eq!(ac_online_from_status(Some("Discharging")), Some(false));
        assert_eq!(ac_online_from_status(Some("Charging")), Some(true));
        assert_eq!(ac_online_from_status(Some("Full")), Some(true));
        assert_eq!(ac_online_from_status(Some("Not charging")), Some(true));
        assert_eq!(ac_online_from_status(Some("Unknown")), None);
        assert_eq!(ac_online_from_status(None), None);
    }

    #[test]
    fn discharge_estimation_from_values() {
        assert_eq!(
            estimate_time_to_empty_seconds(Some("Discharging"), Some(50.0), Some(25.0)),
            Some(7200)
        );
        assert_eq!(
            estimate_time_to_empty_seconds(Some("Charging"), Some(50.0), Some(25.0)),
            None
        );
        assert_eq!(
            estimate_time_to_empty_seconds(Some("Discharging"), Some(0.0), Some(25.0)),
            None
        );
        assert_eq!(
            estimate_time_to_empty_seconds(Some("Discharging"), Some(50.0), Some(0.0)),
            None
        );
        assert_eq!(
            estimate_time_to_empty_seconds(Some("Discharging"), None, Some(25.0)),
            None
        );
    }

    #[test]
    fn attention_tracks_thresholds() {
        assert_eq!(
            attention_for_capacity(10, DEFAULT_WARNING_PERCENT, DEFAULT_DANGER_PERCENT),
            GaugeValueAttention::Danger
        );
        assert_eq!(
            attention_for_capacity(30, DEFAULT_WARNING_PERCENT, DEFAULT_DANGER_PERCENT),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for_capacity(60, DEFAULT_WARNING_PERCENT, DEFAULT_DANGER_PERCENT),
            GaugeValueAttention::Nominal
        );
    }

    #[test]
    fn battery_value_formats_fallback_text() {
        let (value, attention) = battery_value_from_strings(
            Some("abc"),
            Some("Discharging"),
            DEFAULT_WARNING_PERCENT,
            DEFAULT_DANGER_PERCENT,
        )
        .expect("value present");
        assert_eq!(attention, GaugeValueAttention::Nominal);
        match value {
            Some(GaugeValue::Text(text)) => assert_eq!(text, "abc% (Discharging)"),
            _ => panic!("expected text fallback"),
        }
    }

    #[test]
    fn battery_value_uses_icon_when_numeric() {
        let (value, attention) = battery_value_from_strings(
            Some("50"),
            Some("Charging"),
            DEFAULT_WARNING_PERCENT,
            DEFAULT_DANGER_PERCENT,
        )
        .expect("value present");
        assert_eq!(attention, GaugeValueAttention::Nominal);
        match value {
            Some(GaugeValue::Svg(_)) => {}
            _ => panic!("expected svg value"),
        }
    }
}
