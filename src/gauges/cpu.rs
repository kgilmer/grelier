use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, fixed_interval};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use iced::Subscription;
use iced::futures::StreamExt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

fn attention_for(utilization: f32) -> GaugeValueAttention {
    if utilization > 0.90 {
        GaugeValueAttention::Danger
    } else if utilization > 0.75 {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

#[derive(Default)]
struct CpuState {
    previous: Option<CpuTime>,
    fast_interval: bool,
    below_threshold_streak: u8,
}

impl CpuState {
    fn update_interval_state(&mut self, utilization: f32) {
        if utilization > 0.5 {
            self.fast_interval = true;
            self.below_threshold_streak = 0;
        } else if self.fast_interval {
            self.below_threshold_streak = self.below_threshold_streak.saturating_add(1);
            // Relax back to a slower interval after a handful of calm ticks.
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

fn cpu_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let state = Arc::new(Mutex::new(CpuState::default()));
    let interval_state = Arc::clone(&state);

    fixed_interval(
        "cpu",
        Some(svg_asset("microchip.svg")),
        move || {
            interval_state
                .lock()
                .map(|s| s.interval())
                .unwrap_or(Duration::from_secs(2))
        },
        move || {
            let now = read_cpu_time()?;

            let mut state = state.lock().ok()?;
            let previous = match state.previous {
                Some(prev) => prev,
                None => {
                    state.previous = Some(now);
                    return Some((
                        GaugeValue::Svg(icon_quantity(QuantityStyle::Grid, 0.0)),
                        GaugeValueAttention::Nominal,
                    ));
                }
            };

            let utilization = now.utilization_since(previous);
            state.previous = Some(now);
            state.update_interval_state(utilization);

            let attention = attention_for(utilization);
            Some((
                GaugeValue::Svg(icon_quantity(QuantityStyle::Grid, utilization)),
                attention,
            ))
        },
        None,
    )
}

pub fn cpu_subscription() -> Subscription<Message> {
    Subscription::run(|| cpu_stream().map(Message::Gauge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_interval_speeds_up_and_recovers() {
        let mut state = CpuState::default();

        // Jump to fast interval when utilization crosses threshold.
        state.update_interval_state(0.6);
        assert_eq!(state.interval(), Duration::from_secs(1));

        // Stay fast for several below-threshold ticks.
        for _ in 0..3 {
            state.update_interval_state(0.4);
            assert_eq!(state.interval(), Duration::from_secs(1));
        }

        // Recover to slow interval after the 4th below-threshold tick.
        state.update_interval_state(0.4);
        assert_eq!(state.interval(), Duration::from_secs(4));
    }
}
