use crate::gauge::{GaugeModel, event_stream};

/// Stream battery information via udev power_supply events.
pub fn battery_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    event_stream("battery", Some("battery"), |mut sender| {
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

            if let Some(value) = battery_value(&device) {
                let _ = sender.try_send(GaugeModel {
                    id: "battery".into(),
                    title: Some("battery".into()),
                    value,
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
        if let Some(value) = battery_value(&dev) {
            let _ = sender.try_send(GaugeModel {
                id: "battery".into(),
                title: Some("battery".into()),
                value,
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

fn battery_value(dev: &udev::Device) -> Option<String> {
    let capacity =
        property_str(dev, "POWER_SUPPLY_CAPACITY").or_else(|| property_str(dev, "CAPACITY"));
    let status = property_str(dev, "POWER_SUPPLY_STATUS");

    match (capacity, status) {
        (Some(cap), Some(status)) => Some(format!("{cap}% ({status})")),
        (Some(cap), None) => Some(format!("{cap}%")),
        _ => None,
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
