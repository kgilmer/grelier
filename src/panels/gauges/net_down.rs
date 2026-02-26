// Download rate gauge backed by the shared network sampler.
// Consumes Settings: grelier.gauge.net.* (via net_common).
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{
    GaugeDisplay, GaugeInteractionModel, GaugeModel, GaugePointerInteraction, GaugeValue,
    GaugeValueAttention,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::panels::gauges::net_common::{
    NetIntervalState, SlidingWindow, format_rate_per_sec, net_interval_config_from_settings,
    shared_net_sampler,
};
use crate::settings::{NO_SETTINGS, SettingSpec};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const RATE_WINDOW_SAMPLES: usize = 60;

fn map_rate(rate: Option<f64>, window: &mut SlidingWindow) -> (GaugeDisplay, f64) {
    match rate {
        Some(bytes_per_sec) => {
            let ratio = window.push(bytes_per_sec);
            (
                GaugeDisplay::Value {
                    value: GaugeValue::Svg(icon_quantity(ratio)),
                    attention: GaugeValueAttention::Nominal,
                },
                bytes_per_sec,
            )
        }
        None => (
            GaugeDisplay::Value {
                value: GaugeValue::Svg(icon_quantity(0.0)),
                attention: GaugeValueAttention::Warning,
            },
            0.0,
        ),
    }
}

/// Gauge that displays recent download throughput.
struct NetDownGauge {
    /// Shared network sampler that provides interface rates.
    sampler: Arc<Mutex<crate::panels::gauges::net_common::NetSampler>>,
    /// Adaptive interval controller based on recent throughput.
    interval_state: NetIntervalState,
    /// Sliding window used to smooth and classify sampled rates.
    rate_window: SlidingWindow,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for NetDownGauge {
    fn id(&self) -> &'static str {
        "net_down"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let (rate, iface) = self
            .sampler
            .lock()
            .ok()
            .map(|mut sampler| {
                let rates = sampler.rates();
                let iface = sampler.cached_interface();
                (rates, iface)
            })
            .unwrap_or((None, None));
        let rate = rate.map(|rates| rates.download_bytes_per_sec);
        let (display, bytes_per_sec) = map_rate(rate, &mut self.rate_window);

        self.interval_state.update(bytes_per_sec);
        self.next_deadline = now + self.interval_state.interval();

        Some(GaugeModel {
            id: "net_down",
            icon: svg_asset("download.svg"),
            display,
            interactions: GaugeInteractionModel {
                left_click: GaugePointerInteraction {
                    info: Some(InfoDialog {
                        title: "Net Down".to_string(),
                        lines: vec![
                            iface.unwrap_or_else(|| "No active interface".to_string()),
                            format_rate_per_sec(bytes_per_sec),
                        ],
                    }),
                    ..GaugePointerInteraction::default()
                },
                ..GaugeInteractionModel::default()
            },
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    Box::new(NetDownGauge {
        sampler: shared_net_sampler(),
        interval_state: NetIntervalState::new(net_interval_config_from_settings()),
        rate_window: SlidingWindow::new(RATE_WINDOW_SAMPLES),
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    NO_SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "net_down",
        description: "Network download rate gauge displaying a relative icon.",
        default_enabled: false,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_on_missing_rate() {
        let mut window = SlidingWindow::new(RATE_WINDOW_SAMPLES);
        let (display, bytes) = map_rate(None, &mut window);
        let GaugeDisplay::Value {
            value: GaugeValue::Svg(handle),
            attention,
        } = display
        else {
            panic!("expected svg value for missing rate");
        };
        assert_eq!(handle, icon_quantity(0.0));
        assert_eq!(attention, GaugeValueAttention::Warning);
        assert_eq!(bytes, 0.0);
    }
}
