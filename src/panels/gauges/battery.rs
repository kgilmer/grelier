// Battery gauge driven by udev power_supply events and snapshots.
// Consumes Settings: grelier.gauge.battery.warning_percent, grelier.gauge.battery.danger_percent.
use crate::icon::svg_asset;
use crate::info_dialog::InfoDialog;
use crate::panels::gauges::gauge::{GaugeModel, GaugeValue, GaugeValueAttention, event_stream};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings;
use crate::settings::SettingSpec;
use battery::State as BatteryState;
use battery::units::{energy::watt_hour, time::second};
use std::sync::{Arc, Mutex};

const DEFAULT_WARNING_PERCENT: u8 = 49;
const DEFAULT_DANGER_PERCENT: u8 = 19;

/// Stream battery information via udev power_supply events.
fn battery_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("battery", None, |mut sender| {
        let warning_percent = settings::settings().get_parsed_or(
            "grelier.gauge.battery.warning_percent",
            DEFAULT_WARNING_PERCENT,
        );
        let danger_percent = settings::settings().get_parsed_or(
            "grelier.gauge.battery.danger_percent",
            DEFAULT_DANGER_PERCENT,
        );
        let manager = battery::Manager::new().ok();
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
            &manager,
        );

        // Try to open a udev monitor; if it fails, just exit the worker.
        let monitor = match udev::MonitorBuilder::new()
            .and_then(|m| m.match_subsystem("power_supply"))
            .and_then(|m| m.listen())
        {
            Ok(m) => m,
            Err(err) => {
                eprintln!("battery gauge: failed to start udev monitor: {err}");
                return;
            }
        };

        for event in monitor.iter() {
            let device = event.device();
            if !is_battery(&device) {
                continue;
            }

            update_info_state(&info_state, battery_info_dialog(&device, manager.as_ref()));

            if let Some((value, attention)) =
                battery_value(&device, warning_percent, danger_percent)
            {
                let icon = if value.is_some() {
                    None
                } else {
                    Some(svg_asset("battery-alert-variant.svg"))
                };
                let _ = sender.try_send(GaugeModel {
                    id: "battery",
                    icon,
                    value,
                    attention,
                    on_click: None,
                    menu: None,
                    info: info_state.lock().ok().map(|info| info.clone()),
                });
            }
        }
    })
}

fn send_snapshot(
    sender: &mut iced::futures::channel::mpsc::Sender<GaugeModel>,
    warning_percent: u8,
    danger_percent: u8,
    info_state: &Arc<Mutex<InfoDialog>>,
    manager: &Option<battery::Manager>,
) {
    let mut enumerator = match udev::Enumerator::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("battery gauge: failed to enumerate devices: {err}");
            return;
        }
    };

    if enumerator.match_subsystem("power_supply").is_err() {
        eprintln!("battery gauge: failed to set subsystem filter");
        return;
    }

    let devices = match enumerator.scan_devices() {
        Ok(list) => list,
        Err(err) => {
            eprintln!("battery gauge: failed to scan devices: {err}");
            return;
        }
    };

    let mut found_battery = false;

    for dev in devices {
        if !is_battery(&dev) {
            continue;
        }
        found_battery = true;
        update_info_state(info_state, battery_info_dialog(&dev, manager.as_ref()));
        if let Some((value, attention)) = battery_value(&dev, warning_percent, danger_percent) {
            let icon = if value.is_some() {
                None
            } else {
                Some(svg_asset("battery-alert-variant.svg"))
            };
            let _ = sender.try_send(GaugeModel {
                id: "battery",
                icon,
                value,
                attention,
                on_click: None,
                menu: None,
                info: info_state.lock().ok().map(|info| info.clone()),
            });
        }
    }

    if !found_battery {
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
        let _ = sender.try_send(GaugeModel {
            id: "battery",
            icon: Some(svg_asset("battery-alert-variant.svg")),
            value: None,
            attention: GaugeValueAttention::Danger,
            on_click: None,
            menu: None,
            info: info_state.lock().ok().map(|info| info.clone()),
        });
    }
}

fn is_battery(dev: &udev::Device) -> bool {
    dev.property_value("POWER_SUPPLY_TYPE")
        .and_then(|v| v.to_str())
        .map(|v| v.eq_ignore_ascii_case("Battery"))
        .unwrap_or(false)
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
    warning_percent: u8,
    danger_percent: u8,
) -> Option<(Option<GaugeValue>, GaugeValueAttention)> {
    let is_charging = status
        .map(|value| value.eq_ignore_ascii_case("Charging"))
        .unwrap_or(false);

    if let Some(cap) = capacity {
        if let Ok(percent) = cap.parse::<u8>() {
            let attention = attention_for_capacity(percent, warning_percent, danger_percent);
            return Some((
                Some(GaugeValue::Svg(svg_asset(battery_icon(
                    percent,
                    is_charging,
                )))),
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
    let eta_seconds = time_to_empty_seconds(dev);
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

fn battery_icon(percent: u8, is_charging: bool) -> &'static str {
    let step = battery_icon_step(percent, is_charging);
    match (is_charging, step) {
        (true, 10) => "battery-charging-10.svg",
        (true, 20) => "battery-charging-20.svg",
        (true, 30) => "battery-charging-30.svg",
        (true, 40) => "battery-charging-40.svg",
        (true, 50) => "battery-charging-50.svg",
        (true, 60) => "battery-charging-60.svg",
        (true, 70) => "battery-charging-70.svg",
        (true, 80) => "battery-charging-80.svg",
        (true, 90) => "battery-charging-90.svg",
        (true, _) => "battery-charging-100.svg",
        (false, 10) => "battery-10.svg",
        (false, 20) => "battery-20.svg",
        (false, 30) => "battery-30.svg",
        (false, 40) => "battery-40.svg",
        (false, 50) => "battery-50.svg",
        (false, 60) => "battery-60.svg",
        (false, 70) => "battery-70.svg",
        (false, 80) => "battery-80.svg",
        (false, 90) => "battery-90.svg",
        (false, _) => "battery.svg",
    }
}

fn battery_icon_step(percent: u8, allow_full: bool) -> u8 {
    let mut step = u16::from(percent).div_ceil(10) * 10;
    if step < 10 {
        step = 10;
    }
    let max_step = if allow_full { 100 } else { 90 };
    if step > max_step {
        step = max_step;
    }
    step as u8
}

fn attention_for_capacity(
    percent: u8,
    warning_percent: u8,
    danger_percent: u8,
) -> GaugeValueAttention {
    if percent <= danger_percent {
        GaugeValueAttention::Danger
    } else if percent <= warning_percent {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
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
    fn battery_icon_step_clamps_by_mode() {
        assert_eq!(battery_icon_step(1, false), 10);
        assert_eq!(battery_icon_step(95, false), 90);
        assert_eq!(battery_icon_step(95, true), 100);
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
