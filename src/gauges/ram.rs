// RAM utilization gauge with adaptive polling and optional ZFS ARC accounting.
// Consumes Settings: grelier.gauge.ram.*.
use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeInput, GaugeValue, GaugeValueAttention, SettingSpec,
    fixed_interval,
};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use crate::settings;
use iced::mouse;
use std::fs::{File, read_to_string};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_WARNING_THRESHOLD: f32 = 0.85;
const DEFAULT_DANGER_THRESHOLD: f32 = 0.95;
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

fn attention_for(
    utilization: f32,
    warning_threshold: f32,
    danger_threshold: f32,
) -> GaugeValueAttention {
    if utilization > danger_threshold {
        GaugeValueAttention::Danger
    } else if utilization > warning_threshold {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn ram_value(
    utilization: Option<f32>,
    style: QuantityStyle,
    warning_threshold: f32,
    danger_threshold: f32,
) -> (Option<GaugeValue>, GaugeValueAttention) {
    match utilization {
        Some(util) => (
            Some(GaugeValue::Svg(icon_quantity(style, util))),
            attention_for(util, warning_threshold, danger_threshold),
        ),
        None => (None, GaugeValueAttention::Danger),
    }
}

struct RamState {
    fast_interval: bool,
    below_threshold_streak: u8,
    quantity_style: QuantityStyle,
    fast_threshold: f32,
    calm_ticks: u8,
    fast_interval_duration: Duration,
    slow_interval_duration: Duration,
    warning_threshold: f32,
    danger_threshold: f32,
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

fn ram_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let style_value = settings::settings().get_or("grelier.gauge.ram.quantitystyle", "grid");
    let style = QuantityStyle::parse_setting("grelier.gauge.ram.quantitystyle", &style_value);
    let warning_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.ram.warning_threshold",
        DEFAULT_WARNING_THRESHOLD,
    );
    let danger_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.ram.danger_threshold",
        DEFAULT_DANGER_THRESHOLD,
    );
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
    let state = Arc::new(Mutex::new(RamState {
        quantity_style: style,
        fast_threshold,
        calm_ticks,
        fast_interval_duration: Duration::from_secs(fast_interval_secs),
        slow_interval_duration: Duration::from_secs(slow_interval_secs),
        warning_threshold,
        danger_threshold,
        fast_interval: false,
        below_threshold_streak: 0,
    }));
    let on_click: GaugeClickAction = {
        let state = Arc::clone(&state);
        Arc::new(move |click: GaugeClick| {
            if matches!(click.input, GaugeInput::Button(mouse::Button::Left))
                && let Ok(mut state) = state.lock()
            {
                state.quantity_style = state.quantity_style.toggle();
                settings::settings().update(
                    "grelier.gauge.ram.quantitystyle",
                    state.quantity_style.as_setting_value(),
                );
            }
        })
    };

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
            move || {
                let utilization = memory_utilization();
                let mut style = QuantityStyle::Grid;
                if let Ok(mut state) = state.lock() {
                    style = state.quantity_style;
                    if let Some(util) = utilization {
                        state.update_interval_state(util);
                    }
                }

                let (value, attention) =
                    ram_value(utilization, style, warning_threshold, danger_threshold);
                Some((value, attention))
            }
        },
        Some(on_click),
    )
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.ram.quantitystyle",
            default: "grid",
        },
        SettingSpec {
            key: "grelier.gauge.ram.warning_threshold",
            default: "0.85",
        },
        SettingSpec {
            key: "grelier.gauge.ram.danger_threshold",
            default: "0.95",
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
        label: "RAM",
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
    fn attention_thresholds_match_spec() {
        assert_eq!(
            attention_for(0.80, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Nominal
        );
        assert_eq!(
            attention_for(0.86, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for(0.95, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for(0.96, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Danger
        );
    }

    #[test]
    fn ram_interval_speeds_up_and_recovers() {
        let mut state = RamState {
            quantity_style: QuantityStyle::Grid,
            fast_threshold: DEFAULT_FAST_THRESHOLD,
            calm_ticks: DEFAULT_CALM_TICKS,
            fast_interval_duration: Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS),
            slow_interval_duration: Duration::from_secs(DEFAULT_SLOW_INTERVAL_SECS),
            warning_threshold: DEFAULT_WARNING_THRESHOLD,
            danger_threshold: DEFAULT_DANGER_THRESHOLD,
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
            QuantityStyle::Grid,
            DEFAULT_WARNING_THRESHOLD,
            DEFAULT_DANGER_THRESHOLD,
        );
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
    }
}
