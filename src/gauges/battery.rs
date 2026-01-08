use crate::app::Message;
use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention, event_stream};
use crate::icon::svg_asset;
use iced::Subscription;
use iced::futures::StreamExt;

/// Stream battery information via udev power_supply events.
fn battery_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("battery", None, |mut sender| {
        // Send current state so the UI shows something before the first event.
        send_snapshot(&mut sender);

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

            if let Some((value, attention)) = battery_value(&device) {
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

fn send_snapshot(sender: &mut iced::futures::channel::mpsc::Sender<GaugeModel>) {
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
        if let Some((value, attention)) = battery_value(&dev) {
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

fn battery_value(dev: &udev::Device) -> Option<(Option<GaugeValue>, GaugeValueAttention)> {
    let capacity =
        property_str(dev, "POWER_SUPPLY_CAPACITY").or_else(|| property_str(dev, "CAPACITY"));
    let status = property_str(dev, "POWER_SUPPLY_STATUS");
    let is_charging = status
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case("Charging"))
        .unwrap_or(false);

    if let Some(cap) = capacity {
        if let Ok(percent) = cap.parse::<u8>() {
            let attention = attention_for_capacity(percent);
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
    let mut step = ((u16::from(percent) + 9) / 10) * 10;
    if step < 10 {
        step = 10;
    }
    let max_step = if allow_full { 100 } else { 90 };
    if step > max_step {
        step = max_step;
    }
    step as u8
}

fn attention_for_capacity(percent: u8) -> GaugeValueAttention {
    match percent {
        0..=19 => GaugeValueAttention::Danger,
        20..=49 => GaugeValueAttention::Warning,
        _ => GaugeValueAttention::Nominal,
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
