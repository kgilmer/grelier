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
                let _ = sender.try_send(GaugeModel {
                    id: "battery",
                    icon: None,
                    value,
                    attention,
                    on_click: None,
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

    for dev in devices {
        if !is_battery(&dev) {
            continue;
        }
        if let Some((value, attention)) = battery_value(&dev) {
            let _ = sender.try_send(GaugeModel {
                id: "battery",
                icon: None,
                value,
                attention,
                on_click: None,
            });
        }
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

    if let Some(cap) = capacity {
        if let Ok(percent) = cap.parse::<u8>() {
            let attention = attention_for_capacity(percent);
            return Some((Some(GaugeValue::Svg(svg_asset(battery_icon(percent)))), attention));
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

fn battery_icon(percent: u8) -> &'static str {
    match percent {
        0..=19 => "battery-0.svg",
        20..=39 => "battery-2.svg",
        40..=59 => "battery-3.svg",
        60..=79 => "battery-4.svg",
        80..=99 => "battery-5.svg",
        _ => "battery-5.svg",
    }
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
