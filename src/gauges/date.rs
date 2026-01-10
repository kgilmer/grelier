// Date gauge stream that updates daily with month/day formatting.
// Consumes Settings: grelier.date.month_format, grelier.date.day_format.
use chrono::Local;
use iced::Subscription;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval};
use crate::icon::svg_asset;
use crate::settings;

use iced::futures::StreamExt;

const SECS_PER_DAY: u64 = 86_400;
const DAY_LENGTH: Duration = Duration::from_secs(SECS_PER_DAY);
const DEFAULT_MONTH_FORMAT: &str = "%m";
const DEFAULT_DAY_FORMAT: &str = "%d";

/// Stream of the current day (day-of-month, zero-padded) published once per day.
fn day_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let month_format = settings::settings().get_or("grelier.date.month_format", DEFAULT_MONTH_FORMAT);
    let day_format = settings::settings().get_or("grelier.date.day_format", DEFAULT_DAY_FORMAT);
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
        move || {
            let now = Local::now();
            Some((
                Some(GaugeValue::Text(format!(
                    "{}\n{}",
                    now.format(&month_format),
                    now.format(&day_format)
                ))),
                GaugeValueAttention::Nominal,
            ))
        },
        None,
    )
}

pub fn date_subscription() -> Subscription<Message> {
    Subscription::run(|| day_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.date.month_format",
            default: DEFAULT_MONTH_FORMAT,
        },
        SettingSpec {
            key: "grelier.date.day_format",
            default: DEFAULT_DAY_FORMAT,
        },
    ];
    SETTINGS
}
