use crate::app::Message;
use crate::gauge::{fixed_interval, GaugeValue, GaugeValueAttention};
use crate::icon::{icon_quantity, svg_asset, QuantityStyle};
use iced::futures::StreamExt;
use iced::Subscription;
use std::fs::{read_to_string, File};
use std::io::{BufRead, BufReader};
use std::time::Duration;

#[derive(Default)]
struct MemorySnapshot {
    total: u64,
    available: u64,
    free: u64,
    zfs_arc_cache: u64,
    zfs_arc_min: u64,
}

impl MemorySnapshot {
    fn read() -> Option<Self> {
        let file = File::open("/proc/meminfo").ok()?;
        let mut snapshot = MemorySnapshot::default();

        for line in BufReader::new(file).lines() {
            let line = line.ok()?;
            let mut parts = line.split_whitespace();

            let label = match parts.next() {
                Some(label) => label,
                None => continue,
            };
            let value = match parts.next().and_then(|v| v.parse::<u64>().ok()) {
                Some(value) => value.saturating_mul(1024),
                None => continue,
            };

            match label {
                "MemTotal:" => snapshot.total = value,
                "MemAvailable:" => snapshot.available = value,
                "MemFree:" => snapshot.free = value,
                _ => continue,
            }
        }

        snapshot.load_zfs_arc();
        Some(snapshot)
    }

    fn load_zfs_arc(&mut self) {
        let contents = match read_to_string("/proc/spl/kstat/zfs/arcstats") {
            Ok(contents) => contents,
            Err(_) => return,
        };

        for line in contents.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 3 {
                continue;
            }

            match fields[0] {
                "size" => {
                    if let Ok(val) = fields[2].parse::<u64>() {
                        self.zfs_arc_cache = val;
                    }
                }
                "c_min" => {
                    if let Ok(val) = fields[2].parse::<u64>() {
                        self.zfs_arc_min = val;
                    }
                }
                _ => {}
            }
        }
    }

    fn zfs_shrinkable(&self) -> u64 {
        self.zfs_arc_cache.saturating_sub(self.zfs_arc_min)
    }

    fn available_bytes(&self) -> u64 {
        let base_available = if self.available != 0 {
            self.available.min(self.total)
        } else {
            self.free
        };

        base_available.saturating_add(self.zfs_shrinkable())
    }
}

fn memory_utilization() -> Option<f32> {
    let snapshot = MemorySnapshot::read()?;
    if snapshot.total == 0 {
        return None;
    }

    let available = snapshot.available_bytes();
    let used = snapshot.total.saturating_sub(available);
    let utilization = used as f32 / snapshot.total as f32;
    Some(utilization.clamp(0.0, 1.0))
}

fn attention_for(utilization: f32) -> GaugeValueAttention {
    if utilization > 0.90 {
        GaugeValueAttention::Danger
    } else if utilization > 0.75 {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn ram_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    fixed_interval(
        "ram",
        Some(svg_asset("ram.svg")),
        || Duration::from_secs(2),
        || {
            let utilization = memory_utilization()?;
            let attention = attention_for(utilization);

            Some((
                GaugeValue::Svg(icon_quantity(QuantityStyle::Grid, utilization)),
                attention,
            ))
        },
        None,
    )
}

pub fn ram_subscription() -> Subscription<Message> {
    Subscription::run(|| ram_stream().map(Message::Gauge))
}
