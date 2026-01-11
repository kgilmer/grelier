// Battery gauge driven by udev power_supply events and snapshots.
// Consumes Settings: grelier.gauge.battery.warning_percent, grelier.gauge.battery.danger_percent.
use crate::app::Message;
use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention, SettingSpec, event_stream};
use crate::icon::svg_asset;
use crate::settings;
use iced::Subscription;
use iced::futures::StreamExt;

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
        // Send current state so the UI shows something before the first event.
        send_snapshot(&mut sender, warning_percent, danger_percent);

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
                });
            }
        }
    })
}

fn send_snapshot(
    sender: &mut iced::futures::channel::mpsc::Sender<GaugeModel>,
    warning_percent: u8,
    danger_percent: u8,
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
            });
        }
    }

    if !found_battery {
        let _ = sender.try_send(GaugeModel {
            id: "battery",
            icon: Some(svg_asset("battery-alert-variant.svg")),
            value: None,
            attention: GaugeValueAttention::Danger,
            on_click: None,
            menu: None,
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

pub fn battery_subscription() -> Subscription<Message> {
    Subscription::run(|| battery_stream().map(Message::Gauge))
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
