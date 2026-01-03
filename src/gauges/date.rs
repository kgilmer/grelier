use chrono::Local;
use iced::Subscription;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, fixed_interval};
use crate::icon::svg_asset;

use iced::futures::StreamExt;

const SECS_PER_DAY: u64 = 86_400;
const DAY_LENGTH: Duration = Duration::from_secs(SECS_PER_DAY);

/// Stream of the current day (day-of-month, zero-padded) published once per day.
fn day_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    fixed_interval(
        "date",
        Some(svg_asset("calendar-alt.svg")),
        || {
            let now = SystemTime::now();
            if let Ok(elapsed) = now.duration_since(UNIX_EPOCH) {
                let into_day =
                    Duration::new(elapsed.as_secs() % SECS_PER_DAY, elapsed.subsec_nanos());
                let mut sleep_dur = DAY_LENGTH
                    .checked_sub(into_day)
                    .unwrap_or_else(|| Duration::from_secs(0));

                if sleep_dur.is_zero() {
                    sleep_dur = DAY_LENGTH;
                }

                sleep_dur
            } else {
                Duration::from_secs(1)
            }
        },
        || {
            let now = Local::now();
            Some((
                GaugeValue::Text(format!("{}\n{}", now.format("%m"), now.format("%d"))),
                GaugeValueAttention::Nominal,
            ))
        },
    )
}

pub fn date_subscription() -> Subscription<Message> {
    Subscription::run(|| day_stream().map(Message::Gauge))
}
