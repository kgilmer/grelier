use chrono::Local;
use iced::Subscription;
use iced::mouse;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::Message;
use crate::gauge::{
    fixed_interval, GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention,
};
use crate::icon::svg_asset;
use iced::futures::StreamExt;

#[derive(Debug, Clone, Copy, Default)]
enum HourFormat {
    #[default]
    TwentyFour,
    Twelve,
}

impl HourFormat {
    fn toggle(self) -> Self {
        match self {
            HourFormat::TwentyFour => HourFormat::Twelve,
            HourFormat::Twelve => HourFormat::TwentyFour,
        }
    }

    fn format_str(self) -> &'static str {
        match self {
            HourFormat::TwentyFour => "%H",
            HourFormat::Twelve => "%I",
        }
    }
}

/// Stream of the current wall-clock hour/minute, formatted on two lines.
fn seconds_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let format_state = Arc::new(Mutex::new(HourFormat::TwentyFour));
    let on_click: GaugeClickAction = {
        let format_state = Arc::clone(&format_state);
        Arc::new(move |click: GaugeClick| {
            if let mouse::Button::Right = click.button {
                if let Ok(mut format) = format_state.lock() {
                    *format = format.toggle();
                }
            }
        })
    };

    fixed_interval(
        "clock",
        Some(svg_asset("clock.svg")),
        || {
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0));
            // Sleep until the next minute boundary
            let elapsed_in_minute = Duration::new(elapsed.as_secs() % 60, elapsed.subsec_nanos());
            Duration::from_secs(60).saturating_sub(elapsed_in_minute)
        },
        {
            let format_state = Arc::clone(&format_state);
            move || {
                let now = Local::now();
                let hour_format = format_state
                    .lock()
                    .map(|format| format.format_str())
                    .unwrap_or("%H");
                Some((
                    GaugeValue::Text(format!("{}\n{}", now.format(hour_format), now.format("%M"))),
                    GaugeValueAttention::Nominal,
                ))
            }
        },
        Some(on_click),
    )
}

pub fn clock_subscription() -> Subscription<Message> {
    Subscription::run(|| seconds_stream().map(Message::Gauge))
}
