// RAM utilization gauge with adaptive polling and optional ZFS ARC accounting.
// Consumes Settings: grelier.gauge.ram.*.
use crate::icon::{icon_quantity, svg_asset};
use crate::info_dialog::InfoDialog;
use crate::panels::gauges::gauge::{GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings;
use iced::futures::StreamExt;
use std::fs::{File, read_to_string};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_WARNING_THRESHOLD: f32 = 0.10;
const DEFAULT_DANGER_THRESHOLD: f32 = 0.05;
const DEFAULT_FAST_THRESHOLD: f32 = 0.70;
const DEFAULT_FAST_INTERVAL_SECS: u64 = 1;
const DEFAULT_SLOW_INTERVAL_SECS: u64 = 4;
const DEFAULT_CALM_TICKS: u8 = 4;

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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else if value < 10.0 {
        format!("{:.1} {}", value, UNITS[unit])
    } else {
        format!("{:.0} {}", value, UNITS[unit])
    }
}

fn attention_for_free_ratio(
    free_ratio: f32,
    warning_threshold: f32,
    danger_threshold: f32,
) -> GaugeValueAttention {
    if free_ratio < danger_threshold {
        GaugeValueAttention::Danger
    } else if free_ratio < warning_threshold {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn ram_value(
    utilization: Option<f32>,
    free_ratio: Option<f32>,
    warning_threshold: f32,
    danger_threshold: f32,
) -> (Option<GaugeValue>, GaugeValueAttention) {
    let value = utilization.map(|util| GaugeValue::Svg(icon_quantity(util)));
    let attention = match free_ratio {
        Some(free_ratio) => {
            attention_for_free_ratio(free_ratio, warning_threshold, danger_threshold)
        }
        None => GaugeValueAttention::Danger,
    };
    (value, attention)
}

struct RamState {
    fast_interval: bool,
    below_threshold_streak: u8,
    fast_threshold: f32,
    calm_ticks: u8,
    fast_interval_duration: Duration,
    slow_interval_duration: Duration,
}

impl RamState {
    fn update_interval_state(&mut self, utilization: f32) {
        if utilization > self.fast_threshold {
            self.fast_interval = true;
            self.below_threshold_streak = 0;
        } else if self.fast_interval {
            self.below_threshold_streak = self.below_threshold_streak.saturating_add(1);
            if self.below_threshold_streak >= self.calm_ticks {
                self.fast_interval = false;
                self.below_threshold_streak = 0;
            }
        }
    }

    fn interval(&self) -> Duration {
        if self.fast_interval {
            self.fast_interval_duration
        } else {
            self.slow_interval_duration
        }
    }
}

fn ram_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel> {
    let warning_threshold_raw = settings::settings().get_parsed_or(
        "grelier.gauge.ram.warning_threshold",
        DEFAULT_WARNING_THRESHOLD,
    );
    let danger_threshold_raw = settings::settings().get_parsed_or(
        "grelier.gauge.ram.danger_threshold",
        DEFAULT_DANGER_THRESHOLD,
    );
    // Back-compat: older configs stored "used" thresholds (high values). Convert to free ratios.
    let (warning_threshold, danger_threshold) = {
        let warning = if warning_threshold_raw > 0.5 {
            (1.0 - warning_threshold_raw).clamp(0.0, 1.0)
        } else {
            warning_threshold_raw.clamp(0.0, 1.0)
        };
        let danger = if danger_threshold_raw > 0.5 {
            (1.0 - danger_threshold_raw).clamp(0.0, 1.0)
        } else {
            danger_threshold_raw.clamp(0.0, 1.0)
        };
        // Ensure danger is not above warning for free-ratio thresholds.
        if danger > warning {
            (danger, warning)
        } else {
            (warning, danger)
        }
    };
    let fast_threshold = settings::settings()
        .get_parsed_or("grelier.gauge.ram.fast_threshold", DEFAULT_FAST_THRESHOLD);
    let calm_ticks =
        settings::settings().get_parsed_or("grelier.gauge.ram.calm_ticks", DEFAULT_CALM_TICKS);
    let fast_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.ram.fast_interval_secs",
        DEFAULT_FAST_INTERVAL_SECS,
    );
    let slow_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.ram.slow_interval_secs",
        DEFAULT_SLOW_INTERVAL_SECS,
    );
    let info_state = Arc::new(Mutex::new(InfoDialog {
        title: "RAM".to_string(),
        lines: vec![
            "Total: N/A".to_string(),
            "Free: N/A".to_string(),
            "Reserved: N/A".to_string(),
            "Used: N/A".to_string(),
        ],
    }));
    let state = Arc::new(Mutex::new(RamState {
        fast_threshold,
        calm_ticks,
        fast_interval_duration: Duration::from_secs(fast_interval_secs),
        slow_interval_duration: Duration::from_secs(slow_interval_secs),
        fast_interval: false,
        below_threshold_streak: 0,
    }));

    fixed_interval(
        "ram",
        Some(svg_asset("ram.svg")),
        {
            let state = Arc::clone(&state);
            move || {
                state
                    .lock()
                    .map(|s| s.interval())
                    .unwrap_or(Duration::from_secs(4))
            }
        },
        {
            let state = Arc::clone(&state);
            let info_state = Arc::clone(&info_state);
            move || {
                let snapshot = MemorySnapshot::read();
                let utilization = snapshot.as_ref().and_then(|snapshot| {
                    if snapshot.total == 0 {
                        None
                    } else {
                        let available = snapshot.available_bytes();
                        let used = snapshot.total.saturating_sub(available);
                        let utilization = used as f32 / snapshot.total as f32;
                        Some(utilization.clamp(0.0, 1.0))
                    }
                });
                let free_ratio = snapshot.as_ref().and_then(|snapshot| {
                    if snapshot.total == 0 {
                        None
                    } else {
                        let available = snapshot.available_bytes();
                        let ratio = available as f32 / snapshot.total as f32;
                        Some(ratio.clamp(0.0, 1.0))
                    }
                });
                if let Ok(mut info) = info_state.lock() {
                    if let Some(snapshot) = snapshot.as_ref() {
                        let available = snapshot.available_bytes();
                        let reserved = available.saturating_sub(snapshot.free);
                        let used = snapshot.total.saturating_sub(available);
                        info.lines = vec![
                            format!("Total: {}", format_bytes(snapshot.total)),
                            format!("Free: {}", format_bytes(snapshot.free)),
                            format!("Reserved: {}", format_bytes(reserved)),
                            format!("Used: {}", format_bytes(used)),
                        ];
                    } else {
                        info.lines = vec![
                            "Total: N/A".to_string(),
                            "Free: N/A".to_string(),
                            "Reserved: N/A".to_string(),
                            "Used: N/A".to_string(),
                        ];
                    }
                }
                if let Ok(mut state) = state.lock()
                    && let Some(util) = utilization
                {
                    state.update_interval_state(util);
                }

                let (value, attention) =
                    ram_value(utilization, free_ratio, warning_threshold, danger_threshold);
                Some((value, attention))
            }
        },
        None,
    )
    .map({
        let info_state = Arc::clone(&info_state);
        move |mut model| {
            if let Ok(info) = info_state.lock() {
                model.info = Some(info.clone());
            }
            model
        }
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.ram.warning_threshold",
            default: "0.10",
        },
        SettingSpec {
            key: "grelier.gauge.ram.danger_threshold",
            default: "0.05",
        },
        SettingSpec {
            key: "grelier.gauge.ram.fast_threshold",
            default: "0.70",
        },
        SettingSpec {
            key: "grelier.gauge.ram.calm_ticks",
            default: "4",
        },
        SettingSpec {
            key: "grelier.gauge.ram.fast_interval_secs",
            default: "1",
        },
        SettingSpec {
            key: "grelier.gauge.ram.slow_interval_secs",
            default: "4",
        },
    ];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(ram_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "ram",
        description: "RAM usage gauge showing percent memory utilization.",
        default_enabled: true,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_thresholds_match_spec() {
        assert_eq!(
            attention_for_free_ratio(0.20, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Nominal
        );
        assert_eq!(
            attention_for_free_ratio(0.09, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for_free_ratio(0.05, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for_free_ratio(0.04, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Danger
        );
    }

    #[test]
    fn ram_interval_speeds_up_and_recovers() {
        let mut state = RamState {
            fast_threshold: DEFAULT_FAST_THRESHOLD,
            calm_ticks: DEFAULT_CALM_TICKS,
            fast_interval_duration: Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS),
            slow_interval_duration: Duration::from_secs(DEFAULT_SLOW_INTERVAL_SECS),
            fast_interval: false,
            below_threshold_streak: 0,
        };

        // Jump to fast interval when utilization crosses threshold.
        state.update_interval_state(0.8);
        assert_eq!(
            state.interval(),
            Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS)
        );

        // Stay fast for several below-threshold ticks.
        for _ in 0..3 {
            state.update_interval_state(0.5);
            assert_eq!(
                state.interval(),
                Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS)
            );
        }

        // Recover to slow interval after the 4th below-threshold tick.
        state.update_interval_state(0.5);
        assert_eq!(
            state.interval(),
            Duration::from_secs(DEFAULT_SLOW_INTERVAL_SECS)
        );
    }

    #[test]
    fn returns_none_on_missing_utilization() {
        let (value, attention) = ram_value(
            None,
            None,
            DEFAULT_WARNING_THRESHOLD,
            DEFAULT_DANGER_THRESHOLD,
        );
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
    }
}
