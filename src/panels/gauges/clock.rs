// Clock gauge stream with hour format toggling and optional seconds display.
// Consumes Settings: grelier.gauge.clock.hourformat, grelier.gauge.clock.showseconds.
use chrono::Local;
use iced::mouse;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::icon::svg_asset;
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention, fixed_interval,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings;
use crate::settings::SettingSpec;

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

fn hour_format_from_setting() -> HourFormat {
    let value = settings::settings().get_or("grelier.gauge.clock.hourformat", "24");
    match value.as_str() {
        "24" => HourFormat::TwentyFour,
        "12" => HourFormat::Twelve,
        other => {
            panic!(
                "Invalid setting 'grelier.gauge.clock.hourformat': expected 12 or 24, got '{other}'"
            )
        }
    }
}

/// Stream of the current wall-clock hour/minute, formatted on two lines.
fn seconds_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel> {
    let show_seconds = settings::settings().get_bool_or("grelier.gauge.clock.showseconds", false);
    let format_state = Arc::new(Mutex::new(hour_format_from_setting()));
    let on_click: GaugeClickAction = {
        let format_state = Arc::clone(&format_state);
        Arc::new(move |click: GaugeClick| {
            if let crate::panels::gauges::gauge::GaugeInput::Button(button) = click.input
                && let mouse::Button::Right = button
                && let Ok(mut format) = format_state.lock()
            {
                *format = format.toggle();
            }
        })
    };

    fixed_interval(
        "clock",
        Some(svg_asset("clock.svg")),
        move || {
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0));
            let window = if show_seconds { 1 } else { 60 };
            // Sleep until the next window boundary
            let elapsed_in_window =
                Duration::new(elapsed.as_secs() % window, elapsed.subsec_nanos());
            Duration::from_secs(window).saturating_sub(elapsed_in_window)
        },
        {
            let format_state = Arc::clone(&format_state);
            move || {
                let now = Local::now();
                let hour_format = format_state
                    .lock()
                    .map(|format| format.format_str())
                    .unwrap_or("%H");
                let time_text = if show_seconds {
                    format!(
                        "{}\n{}\n{}",
                        now.format(hour_format),
                        now.format("%M"),
                        now.format("%S")
                    )
                } else {
                    format!("{}\n{}", now.format(hour_format), now.format("%M"))
                };
                Some((
                    Some(GaugeValue::Text(time_text)),
                    GaugeValueAttention::Nominal,
                ))
            }
        },
        Some(on_click),
    )
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.clock.showseconds",
            default: "false",
        },
        SettingSpec {
            key: "grelier.gauge.clock.hourformat",
            default: "24",
        },
    ];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(seconds_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "clock",
        description: "Clock gauge showing the local time.",
        default_enabled: true,
        settings,
        stream,
        validate: None,
    }
}
