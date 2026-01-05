use crate::app::Message;
use crate::gauge::{fixed_interval, GaugeValue, GaugeValueAttention};
use crate::gauges::net_common::{format_rate, NetRateTracker, RateDirection};
use crate::icon::svg_asset;
use iced::futures::StreamExt;
use iced::Subscription;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn net_download_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let tracker = Arc::new(Mutex::new(NetRateTracker::new()));

    fixed_interval(
        "net_download",
        Some(svg_asset("download.svg")),
        || Duration::from_secs(1),
        {
            let tracker = Arc::clone(&tracker);
            move || {
                let rate = tracker
                    .lock()
                    .ok()
                    .and_then(|mut t| t.rate(RateDirection::Download));

                match rate {
                    Some(bytes_per_sec) => Some((
                        GaugeValue::Text(format_rate(bytes_per_sec)),
                        GaugeValueAttention::Nominal,
                    )),
                    None => Some((
                        GaugeValue::Text("--".to_string()),
                        GaugeValueAttention::Danger,
                    )),
                }
            }
        },
        None,
    )
}

pub fn net_download_subscription() -> Subscription<Message> {
    Subscription::run(|| net_download_stream().map(Message::Gauge))
}
