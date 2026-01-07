use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, fixed_interval};
use crate::gauges::net_common::{NetIntervalState, format_rate, shared_net_sampler};
use crate::icon::svg_asset;
use iced::Subscription;
use iced::futures::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn net_down_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let sampler = shared_net_sampler();
    let interval_state = Arc::new(Mutex::new(NetIntervalState::default()));

    fixed_interval(
        "net_down",
        Some(svg_asset("download.svg")),
        {
            let state = Arc::clone(&interval_state);
            move || {
                state
                    .lock()
                    .map(|s| s.interval())
                    .unwrap_or(Duration::from_secs(1))
            }
        },
        {
            let sampler = Arc::clone(&sampler);
            let state = Arc::clone(&interval_state);
            move || {
                let rate = sampler
                    .lock()
                    .ok()
                    .and_then(|mut s| s.rates().map(|r| r.download_bytes_per_sec));

                let (value, attention, bytes_per_sec) = match rate {
                    Some(bytes_per_sec) => (
                        GaugeValue::Text(format_rate(bytes_per_sec)),
                        GaugeValueAttention::Nominal,
                        bytes_per_sec,
                    ),
                    None => (
                        GaugeValue::Text("--".to_string()),
                        GaugeValueAttention::Danger,
                        0.0,
                    ),
                };

                if let Ok(mut state) = state.lock() {
                    state.update(bytes_per_sec);
                }

                Some((value, attention))
            }
        },
        None,
    )
}

pub fn net_down_subscription() -> Subscription<Message> {
    Subscription::run(|| net_down_stream().map(Message::Gauge))
}
