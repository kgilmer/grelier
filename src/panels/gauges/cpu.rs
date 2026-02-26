// CPU utilization gauge with adaptive polling and quantity icons.
// Consumes Settings: grelier.gauge.cpu.*.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{
    GaugeDisplay, GaugeInteractionModel, GaugeModel, GaugePointerInteraction, GaugeValue,
    GaugeValueAttention,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

const DEFAULT_WARNING_THRESHOLD: f32 = 0.90;
const DEFAULT_DANGER_THRESHOLD: f32 = 1.0;
const DEFAULT_FAST_THRESHOLD: f32 = 0.50;
const DEFAULT_FAST_INTERVAL_SECS: u64 = 1;
const DEFAULT_SLOW_INTERVAL_SECS: u64 = 4;
const DEFAULT_CALM_TICKS: u8 = 4;

#[derive(Clone, Copy)]
struct CpuTime {
    idle: u64,
    non_idle: u64,
}

impl CpuTime {
    fn utilization_since(&self, previous: Self) -> f32 {
        let total_now = self.idle.saturating_add(self.non_idle);
        let total_prev = previous.idle.saturating_add(previous.non_idle);
        let total_delta = total_now.saturating_sub(total_prev);
        if total_delta == 0 {
            return 0.0;
        }

        let active_delta = self.non_idle.saturating_sub(previous.non_idle);
        (active_delta as f32 / total_delta as f32).clamp(0.0, 1.0)
    }
}

fn read_cpu_time() -> Option<CpuTime> {
    let file = File::open("/proc/stat").ok()?;
    let mut lines = BufReader::new(file).lines();
    let line = lines.next()?.ok()?;

    if !line.starts_with("cpu ") {
        return None;
    }

    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();

    if values.len() < 7 {
        return None;
    }

    let idle = values[3].saturating_add(values[4]);
    let non_idle = values[0]
        .saturating_add(values[1])
        .saturating_add(values[2])
        .saturating_add(values[5])
        .saturating_add(values[6]);

    Some(CpuTime { idle, non_idle })
}

fn read_cpu_model() -> Option<String> {
    let file = File::open("/proc/cpuinfo").ok()?;
    for line in BufReader::new(file).lines() {
        let line = line.ok()?;
        if let Some(rest) = line.strip_prefix("model name") {
            return rest
                .split_once(':')
                .map(|(_, value)| value.trim().to_string());
        }
    }
    None
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

/// Internal sampling and pacing state for the CPU gauge.
struct CpuState {
    /// Previous `/proc/stat` aggregate sample used to compute utilization deltas.
    previous: Option<CpuTime>,
    /// Whether the gauge is currently polling at the fast interval.
    fast_interval: bool,
    /// Consecutive samples below `fast_threshold` while in fast mode.
    below_threshold_streak: u8,
    /// Utilization threshold that triggers fast polling when exceeded.
    fast_threshold: f32,
    /// Number of calm samples required before returning to slow polling.
    calm_ticks: u8,
    /// Poll interval used while in fast mode.
    fast_interval_duration: Duration,
    /// Poll interval used while in normal/slow mode.
    slow_interval_duration: Duration,
    /// Utilization threshold where display attention becomes warning.
    warning_threshold: f32,
    /// Utilization threshold where display attention becomes danger.
    danger_threshold: f32,
}

impl CpuState {
    fn update_interval_state(&mut self, utilization: f32) {
        if utilization > self.fast_threshold {
            self.fast_interval = true;
            self.below_threshold_streak = 0;
        } else if self.fast_interval {
            self.below_threshold_streak = self.below_threshold_streak.saturating_add(1);
            // Relax back to a slower interval after a handful of calm ticks.
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

fn cpu_value(
    utilization: Option<f32>,
    warning_threshold: f32,
    danger_threshold: f32,
) -> GaugeDisplay {
    match utilization {
        Some(util) => GaugeDisplay::Value {
            value: GaugeValue::Svg(icon_quantity(util)),
            attention: attention_for(util, warning_threshold, danger_threshold),
        },
        None => GaugeDisplay::Error,
    }
}

/// Gauge that samples and displays overall CPU utilization.
struct CpuGauge {
    /// Rolling CPU sample state used to compute utilization and poll intervals.
    state: CpuState,
    /// Human-readable CPU model shown in the info dialog.
    cpu_model: String,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for CpuGauge {
    fn id(&self) -> &'static str {
        "cpu"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let (display, load_line) = match read_cpu_time() {
            Some(current) => match self.state.previous {
                Some(previous) => {
                    let utilization = current.utilization_since(previous);
                    self.state.previous = Some(current);
                    self.state.update_interval_state(utilization);
                    (
                        cpu_value(
                            Some(utilization),
                            self.state.warning_threshold,
                            self.state.danger_threshold,
                        ),
                        format!("Load: {:.1}%", (utilization * 100.0).clamp(0.0, 100.0)),
                    )
                }
                None => {
                    self.state.previous = Some(current);
                    (
                        GaugeDisplay::Value {
                            value: GaugeValue::Svg(icon_quantity(0.0)),
                            attention: GaugeValueAttention::Nominal,
                        },
                        "Load: 0.0%".to_string(),
                    )
                }
            },
            None => (
                cpu_value(
                    None,
                    self.state.warning_threshold,
                    self.state.danger_threshold,
                ),
                "Load: N/A".to_string(),
            ),
        };

        self.next_deadline = now + self.state.interval();

        Some(GaugeModel {
            id: "cpu",
            icon: svg_asset("microchip.svg"),
            display,
            interactions: GaugeInteractionModel {
                left_click: GaugePointerInteraction {
                    info: Some(InfoDialog {
                        title: "CPU".to_string(),
                        lines: vec![self.cpu_model.clone(), load_line],
                    }),
                    ..GaugePointerInteraction::default()
                },
                ..GaugeInteractionModel::default()
            },
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    let warning_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.cpu.warning_threshold",
        DEFAULT_WARNING_THRESHOLD,
    );
    let danger_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.cpu.danger_threshold",
        DEFAULT_DANGER_THRESHOLD,
    );
    let fast_threshold = settings::settings()
        .get_parsed_or("grelier.gauge.cpu.fast_threshold", DEFAULT_FAST_THRESHOLD);
    let calm_ticks =
        settings::settings().get_parsed_or("grelier.gauge.cpu.calm_ticks", DEFAULT_CALM_TICKS);
    let fast_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.cpu.fast_interval_secs",
        DEFAULT_FAST_INTERVAL_SECS,
    );
    let slow_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.cpu.slow_interval_secs",
        DEFAULT_SLOW_INTERVAL_SECS,
    );

    Box::new(CpuGauge {
        state: CpuState {
            previous: None,
            fast_interval: false,
            below_threshold_streak: 0,
            fast_threshold,
            calm_ticks,
            fast_interval_duration: Duration::from_secs(fast_interval_secs),
            slow_interval_duration: Duration::from_secs(slow_interval_secs),
            warning_threshold,
            danger_threshold,
        },
        cpu_model: read_cpu_model().unwrap_or_else(|| "Unknown CPU".to_string()),
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.cpu.warning_threshold",
            default: "0.75",
        },
        SettingSpec {
            key: "grelier.gauge.cpu.danger_threshold",
            default: "0.90",
        },
        SettingSpec {
            key: "grelier.gauge.cpu.fast_threshold",
            default: "0.50",
        },
        SettingSpec {
            key: "grelier.gauge.cpu.calm_ticks",
            default: "4",
        },
        SettingSpec {
            key: "grelier.gauge.cpu.fast_interval_secs",
            default: "1",
        },
        SettingSpec {
            key: "grelier.gauge.cpu.slow_interval_secs",
            default: "4",
        },
    ];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "cpu",
        description: "CPU utilization gauge displaying percent usage with adaptive polling.",
        default_enabled: true,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_interval_speeds_up_and_recovers() {
        let mut state = CpuState {
            fast_threshold: DEFAULT_FAST_THRESHOLD,
            calm_ticks: DEFAULT_CALM_TICKS,
            fast_interval_duration: Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS),
            slow_interval_duration: Duration::from_secs(DEFAULT_SLOW_INTERVAL_SECS),
            warning_threshold: DEFAULT_WARNING_THRESHOLD,
            danger_threshold: DEFAULT_DANGER_THRESHOLD,
            previous: None,
            fast_interval: false,
            below_threshold_streak: 0,
        };

        // Jump to fast interval when utilization crosses threshold.
        state.update_interval_state(0.6);
        assert_eq!(
            state.interval(),
            Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS)
        );

        // Stay fast for several below-threshold ticks.
        for _ in 0..3 {
            state.update_interval_state(0.4);
            assert_eq!(
                state.interval(),
                Duration::from_secs(DEFAULT_FAST_INTERVAL_SECS)
            );
        }

        // Recover to slow interval after the 4th below-threshold tick.
        state.update_interval_state(0.4);
        assert_eq!(
            state.interval(),
            Duration::from_secs(DEFAULT_SLOW_INTERVAL_SECS)
        );
    }

    #[test]
    fn returns_none_on_missing_utilization() {
        assert!(matches!(
            super::cpu_value(None, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeDisplay::Error
        ));
    }
}
