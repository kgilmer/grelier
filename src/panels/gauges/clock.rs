// Clock gauge stream with hour format toggling and optional text/seconds display.
// Consumes Settings: grelier.gauge.clock.hourformat, grelier.gauge.clock.showseconds, grelier.gauge.clock.show_text.
use chrono::Local;
use chrono::Timelike;
use iced::mouse;
use iced::widget::svg;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::panels::gauges::gauge::{Gauge, GaugeReadyNotify};
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
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

#[derive(Debug, Clone)]
struct ClockIconState {
    minute_key: u32,
    handle: svg::Handle,
}

fn clock_icon_for_time(hour: u32, minute: u32) -> svg::Handle {
    const CENTER: f32 = 256.0;
    // Keep the face slightly inside the viewBox to avoid edge clipping alias artifacts.
    const FACE_INSET: f32 = 1.0;
    const FACE_SIZE: f32 = 510.0;
    const FACE_CORNER_RADIUS: f32 = FACE_SIZE * (3.0 / 18.0);
    const HOUR_RADIUS: f32 = 136.0;
    const MINUTE_RADIUS: f32 = 200.0;
    const HOUR_WIDTH: f32 = 48.0;
    const MINUTE_WIDTH: f32 = 40.0;
    const MINUTE_CORE_WIDTH: f32 = 15.0;
    const HOUR_TIP_RADIUS: f32 = 13.0;
    const HOUR_TIP_STROKE: f32 = 10.0;
    const CENTER_DOT_RADIUS: f32 = 18.0;
    const TOP_MARK_WIDTH: f32 = 28.0;
    const TOP_MARK_START_Y: f32 = 12.0;
    const TOP_MARK_LENGTH: f32 = 56.0;
    const TOP_MARK_END_Y: f32 = TOP_MARK_START_Y + TOP_MARK_LENGTH;
    const RIGHT_MARK_START_X: f32 = 500.0;
    const RIGHT_MARK_END_X: f32 = RIGHT_MARK_START_X - TOP_MARK_LENGTH;
    const BOTTOM_MARK_START_Y: f32 = 500.0;
    const BOTTOM_MARK_END_Y: f32 = BOTTOM_MARK_START_Y - TOP_MARK_LENGTH;
    const LEFT_MARK_START_X: f32 = 12.0;
    const LEFT_MARK_END_X: f32 = LEFT_MARK_START_X + TOP_MARK_LENGTH;

    let hour_angle = (((hour % 12) as f32) + (minute as f32 / 60.0)) * 30.0;
    let minute_angle = (minute as f32) * 6.0;

    let (hour_x, hour_y) = hand_endpoint(hour_angle, HOUR_RADIUS);
    let (minute_x, minute_y) = hand_endpoint(minute_angle, MINUTE_RADIUS);
    let svg_data = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" shape-rendering="geometricPrecision">
  <defs>
    <linearGradient id="grelierGaugeGrad" x1="1" y1="0" x2="0" y2="0">
      <stop offset="0%" stop-color="currentColor" stop-opacity="0.7"/>
      <stop offset="100%" stop-color="currentColor" stop-opacity="1"/>
    </linearGradient>
    <mask id="clockHandCutout">
      <rect x="0" y="0" width="512" height="512" fill="white"/>
      <line x1="{CENTER}" y1="{TOP_MARK_START_Y}" x2="{CENTER}" y2="{TOP_MARK_END_Y}" stroke="black" stroke-width="{TOP_MARK_WIDTH}" stroke-linecap="round"/>
      <line x1="{RIGHT_MARK_START_X}" y1="{CENTER}" x2="{RIGHT_MARK_END_X}" y2="{CENTER}" stroke="black" stroke-width="{TOP_MARK_WIDTH}" stroke-linecap="round"/>
      <line x1="{CENTER}" y1="{BOTTOM_MARK_START_Y}" x2="{CENTER}" y2="{BOTTOM_MARK_END_Y}" stroke="black" stroke-width="{TOP_MARK_WIDTH}" stroke-linecap="round"/>
      <line x1="{LEFT_MARK_START_X}" y1="{CENTER}" x2="{LEFT_MARK_END_X}" y2="{CENTER}" stroke="black" stroke-width="{TOP_MARK_WIDTH}" stroke-linecap="round"/>
      <line x1="{CENTER}" y1="{CENTER}" x2="{hour_x:.2}" y2="{hour_y:.2}" stroke="black" stroke-width="{HOUR_WIDTH}" stroke-linecap="round"/>
      <line x1="{CENTER}" y1="{CENTER}" x2="{minute_x:.2}" y2="{minute_y:.2}" stroke="black" stroke-width="{MINUTE_WIDTH}" stroke-linecap="round"/>
      <line x1="{CENTER}" y1="{CENTER}" x2="{minute_x:.2}" y2="{minute_y:.2}" stroke="black" stroke-width="{MINUTE_CORE_WIDTH}" stroke-linecap="round"/>
      <circle cx="{hour_x:.2}" cy="{hour_y:.2}" r="{HOUR_TIP_RADIUS}" fill="none" stroke="black" stroke-width="{HOUR_TIP_STROKE}"/>
      <circle cx="{CENTER}" cy="{CENTER}" r="{CENTER_DOT_RADIUS}" fill="black"/>
    </mask>
  </defs>
  <rect x="{FACE_INSET}" y="{FACE_INSET}" width="{FACE_SIZE}" height="{FACE_SIZE}" rx="{FACE_CORNER_RADIUS}" ry="{FACE_CORNER_RADIUS}" fill="url(#grelierGaugeGrad)" mask="url(#clockHandCutout)" shape-rendering="geometricPrecision"/>
</svg>"##
    );
    svg::Handle::from_memory(svg_data.into_bytes())
}

fn hand_endpoint(angle_degrees: f32, radius: f32) -> (f32, f32) {
    let angle_radians = (angle_degrees - 90.0) * (PI / 180.0);
    (
        256.0 + radius * angle_radians.cos(),
        256.0 + radius * angle_radians.sin(),
    )
}

fn duration_until_window_boundary(window_secs: u64) -> Duration {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let elapsed_in_window = Duration::new(elapsed.as_secs() % window_secs, elapsed.subsec_nanos());
    let sleep = Duration::from_secs(window_secs).saturating_sub(elapsed_in_window);
    if sleep.is_zero() {
        Duration::from_secs(window_secs)
    } else {
        sleep
    }
}

/// Stream of the current wall-clock hour/minute, formatted on two lines.
struct ClockGauge {
    show_seconds: bool,
    show_text: bool,
    format_state: Arc<Mutex<HourFormat>>,
    icon_state: Option<ClockIconState>,
    ready_notify: Option<GaugeReadyNotify>,
    next_deadline: Instant,
}

impl Gauge for ClockGauge {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn bind_ready_notify(&mut self, notify: GaugeReadyNotify) {
        self.ready_notify = Some(notify);
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let local_now = Local::now();
        let minute_key = local_now.hour() * 60 + local_now.minute();
        let icon = if self
            .icon_state
            .as_ref()
            .map(|cached| cached.minute_key != minute_key)
            .unwrap_or(true)
        {
            let icon = clock_icon_for_time(local_now.hour(), local_now.minute());
            self.icon_state = Some(ClockIconState {
                minute_key,
                handle: icon.clone(),
            });
            icon
        } else {
            self.icon_state
                .as_ref()
                .map(|cached| cached.handle.clone())
                .unwrap_or_else(|| clock_icon_for_time(local_now.hour(), local_now.minute()))
        };

        let display = if self.show_text {
            let hour_format = self
                .format_state
                .lock()
                .map(|format| format.format_str())
                .unwrap_or("%H");
            let time_text = if self.show_seconds {
                format!(
                    "{}\n{}\n{}",
                    local_now.format(hour_format),
                    local_now.format("%M"),
                    local_now.format("%S")
                )
            } else {
                format!(
                    "{}\n{}",
                    local_now.format(hour_format),
                    local_now.format("%M")
                )
            };
            GaugeDisplay::Value {
                value: GaugeValue::Text(time_text),
                attention: GaugeValueAttention::Nominal,
            }
        } else {
            GaugeDisplay::Empty
        };

        let format_state = Arc::clone(&self.format_state);
        let ready_notify = self.ready_notify.clone();
        let on_click: GaugeClickAction = Arc::new(move |click: GaugeClick| {
            if let crate::panels::gauges::gauge::GaugeInput::Button(button) = click.input
                && let mouse::Button::Right = button
                && let Ok(mut format) = format_state.lock()
            {
                *format = format.toggle();
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("clock");
                }
            }
        });

        let interval = if self.show_text && self.show_seconds {
            1
        } else {
            60
        };
        self.next_deadline = now + duration_until_window_boundary(interval);

        Some(GaugeModel {
            id: "clock",
            icon: Some(icon),
            display,
            on_click: Some(on_click),
            menu: None,
            action_dialog: None,
            info: None,
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    Box::new(ClockGauge {
        show_seconds: settings::settings().get_bool_or("grelier.gauge.clock.showseconds", false),
        show_text: settings::settings().get_bool_or("grelier.gauge.clock.show_text", true),
        format_state: Arc::new(Mutex::new(hour_format_from_setting())),
        icon_state: None,
        ready_notify: None,
        next_deadline: now,
    })
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
        SettingSpec {
            key: "grelier.gauge.clock.show_text",
            default: "true",
        },
    ];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "clock",
        description: "Clock gauge showing the local time.",
        default_enabled: true,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "expected {a} ~= {b}");
    }

    #[test]
    fn hand_endpoint_points_up_at_zero_degrees() {
        let (x, y) = hand_endpoint(0.0, 10.0);
        approx_eq(x, 256.0);
        approx_eq(y, 246.0);
    }

    #[test]
    fn hand_endpoint_points_right_at_ninety_degrees() {
        let (x, y) = hand_endpoint(90.0, 10.0);
        approx_eq(x, 266.0);
        approx_eq(y, 256.0);
    }

    #[test]
    fn clock_icon_contains_expected_hand_positions() {
        let handle = clock_icon_for_time(3, 0);
        let data = match handle.data() {
            iced_core::svg::Data::Bytes(bytes) => {
                std::str::from_utf8(bytes).expect("svg data should be utf-8")
            }
            _ => panic!("expected in-memory SVG bytes"),
        };
        assert!(
            data.contains("x2=\"392.00\" y2=\"256.00\""),
            "hour hand at 3:00"
        );
        assert!(
            data.contains("x2=\"256.00\" y2=\"56.00\""),
            "minute hand at 0 minutes"
        );
        assert!(
            data.contains("width=\"510\" height=\"510\" rx=\"85\" ry=\"85\""),
            "face is rendered as an inset rounded square matching quantity icon corner ratio"
        );
        assert!(
            data.contains(
                "x1=\"256\" y1=\"12\" x2=\"256\" y2=\"68\" stroke=\"black\" stroke-width=\"28\""
            ),
            "mask includes thicker and longer top-center inverse 12 o'clock marker"
        );
        assert!(
            data.contains(
                "x1=\"500\" y1=\"256\" x2=\"444\" y2=\"256\" stroke=\"black\" stroke-width=\"28\""
            ),
            "mask includes inverse 3 o'clock marker"
        );
        assert!(
            data.contains(
                "x1=\"256\" y1=\"500\" x2=\"256\" y2=\"444\" stroke=\"black\" stroke-width=\"28\""
            ),
            "mask includes inverse 6 o'clock marker"
        );
        assert!(
            data.contains(
                "x1=\"12\" y1=\"256\" x2=\"68\" y2=\"256\" stroke=\"black\" stroke-width=\"28\""
            ),
            "mask includes inverse 9 o'clock marker"
        );
        assert!(
            data.contains("shape-rendering=\"geometricPrecision\""),
            "svg requests antialiased geometry rendering"
        );
    }

    #[test]
    fn settings_include_show_text_default_true() {
        let clock_settings = settings();
        let show_text = clock_settings
            .iter()
            .find(|spec| spec.key == "grelier.gauge.clock.show_text")
            .expect("show_text setting should exist");
        assert_eq!(show_text.default, "true");
    }
}
