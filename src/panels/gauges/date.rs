// Date gauge stream that updates daily with month/day formatting.
// Consumes Settings: grelier.gauge.date.month_format, grelier.gauge.date.day_format.
use chrono::Local;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::icon::svg_asset;
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;

const SECS_PER_DAY: u64 = 86_400;
const DAY_LENGTH: Duration = Duration::from_secs(SECS_PER_DAY);
const DEFAULT_MONTH_FORMAT: &str = "%m";
const DEFAULT_DAY_FORMAT: &str = "%d";

fn day_rollover_delay() -> Duration {
    let now = SystemTime::now();
    if let Ok(elapsed) = now.duration_since(UNIX_EPOCH) {
        let into_day = Duration::new(elapsed.as_secs() % SECS_PER_DAY, elapsed.subsec_nanos());
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
}

fn render_date_display(month_format: &str, day_format: &str) -> GaugeDisplay {
    let now = Local::now();
    GaugeDisplay::Value {
        value: GaugeValue::Text(format!(
            "{}\n{}",
            now.format(month_format),
            now.format(day_format)
        )),
        attention: GaugeValueAttention::Nominal,
    }
}

/// Stream of the current day (day-of-month, zero-padded) published once per day.
struct DateGauge {
    /// `chrono` format string used for the month portion of the display.
    month_format: String,
    /// `chrono` format string used for the day portion of the display.
    day_format: String,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for DateGauge {
    fn id(&self) -> &'static str {
        "date"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        self.next_deadline = now + day_rollover_delay();
        Some(GaugeModel {
            id: "date",
            icon: svg_asset("calendar-alt.svg"),
            display: render_date_display(&self.month_format, &self.day_format),
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    let month_format =
        settings::settings().get_or("grelier.gauge.date.month_format", DEFAULT_MONTH_FORMAT);
    let day_format =
        settings::settings().get_or("grelier.gauge.date.day_format", DEFAULT_DAY_FORMAT);
    Box::new(DateGauge {
        month_format,
        day_format,
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.date.month_format",
            default: DEFAULT_MONTH_FORMAT,
        },
        SettingSpec {
            key: "grelier.gauge.date.day_format",
            default: DEFAULT_DAY_FORMAT,
        },
    ];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "date",
        description: "Date gauge showing the current calendar date.",
        default_enabled: true,
        settings,
        create: create_gauge,
        validate: None,
    }
}
