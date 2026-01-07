use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, fixed_interval};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use iced::Subscription;
use iced::futures::StreamExt;
use std::fs::{File, read_to_string};
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
    if utilization > 0.95 {
        GaugeValueAttention::Danger
    } else if utilization > 0.85 {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn ram_value(utilization: Option<f32>) -> (Option<GaugeValue>, GaugeValueAttention) {
    match utilization {
        Some(util) => (
            Some(GaugeValue::Svg(icon_quantity(
                QuantityStyle::Grid,
                util,
            ))),
            attention_for(util),
        ),
        None => (None, GaugeValueAttention::Danger),
    }
}

#[derive(Default)]
struct RamState {
    fast_interval: bool,
    below_threshold_streak: u8,
}

impl RamState {
    fn update_interval_state(&mut self, utilization: f32) {
        if utilization > 0.70 {
            self.fast_interval = true;
            self.below_threshold_streak = 0;
        } else if self.fast_interval {
            self.below_threshold_streak = self.below_threshold_streak.saturating_add(1);
            if self.below_threshold_streak > 3 {
                self.fast_interval = false;
                self.below_threshold_streak = 0;
            }
        }
    }

    fn interval(&self) -> Duration {
        if self.fast_interval {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(4)
        }
    }
}

fn ram_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let state = std::sync::Arc::new(std::sync::Mutex::new(RamState::default()));

    fixed_interval(
        "ram",
        Some(svg_asset("ram.svg")),
        {
            let state = std::sync::Arc::clone(&state);
            move || {
                state
                    .lock()
                    .map(|s| s.interval())
                    .unwrap_or(Duration::from_secs(4))
            }
        },
        {
            let state = std::sync::Arc::clone(&state);
            move || {
                let utilization = memory_utilization();
                let (value, attention) = ram_value(utilization);
                if let Ok(mut state) = state.lock() {
                    if let Some(util) = utilization {
                        state.update_interval_state(util);
                    }
                }

                Some((value, attention))
            }
        },
        None,
    )
}

pub fn ram_subscription() -> Subscription<Message> {
    Subscription::run(|| ram_stream().map(Message::Gauge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_thresholds_match_spec() {
        assert_eq!(attention_for(0.80), GaugeValueAttention::Nominal);
        assert_eq!(attention_for(0.86), GaugeValueAttention::Warning);
        assert_eq!(attention_for(0.95), GaugeValueAttention::Warning);
        assert_eq!(attention_for(0.96), GaugeValueAttention::Danger);
    }

    #[test]
    fn ram_interval_speeds_up_and_recovers() {
        let mut state = RamState::default();

        // Jump to fast interval when utilization crosses threshold.
        state.update_interval_state(0.8);
        assert_eq!(state.interval(), Duration::from_secs(1));

        // Stay fast for several below-threshold ticks.
        for _ in 0..3 {
            state.update_interval_state(0.5);
            assert_eq!(state.interval(), Duration::from_secs(1));
        }

        // Recover to slow interval after the 4th below-threshold tick.
        state.update_interval_state(0.5);
        assert_eq!(state.interval(), Duration::from_secs(4));
    }

    #[test]
    fn returns_none_on_missing_utilization() {
        let (value, attention) = ram_value(None);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
    }
}
