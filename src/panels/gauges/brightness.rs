// Backlight brightness gauge with scroll adjustments via sysfs.
// Consumes Settings: grelier.gauge.brightness.step_percent, grelier.gauge.brightness.refresh_interval_secs.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{Gauge, GaugeReadyNotify};
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeInput, GaugeValue, GaugeValueAttention,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self};
use std::time::{Duration, Instant};

const DEFAULT_STEP_PERCENT: i8 = 5;
const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 2;
const ABS_MAX_PERCENT: u8 = 100;
const SYS_BACKLIGHT: &str = "/sys/class/backlight";

fn brightness_value(percent: Option<u8>) -> GaugeDisplay {
    match percent {
        Some(p) => GaugeDisplay::Value {
            value: GaugeValue::Svg(icon_quantity(p as f32 / ABS_MAX_PERCENT as f32)),
            attention: GaugeValueAttention::Nominal,
        },
        None => GaugeDisplay::Error,
    }
}

fn read_u32(path: &Path) -> io::Result<u32> {
    let contents = fs::read_to_string(path)?;
    contents
        .split_whitespace()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing value"))
        .and_then(|s| {
            s.parse::<u32>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
}

fn percent_from_raw(raw: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }

    let ratio = raw as f64 / max as f64;
    (ratio * 100.0).round().clamp(0.0, ABS_MAX_PERCENT as f64) as u8
}

fn raw_from_percent(percent: u8, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }

    let clamped = percent.min(ABS_MAX_PERCENT) as u64;
    (((clamped * max as u64) + 50) / 100) as u32
}

#[derive(Debug, Clone)]
struct Backlight {
    brightness: PathBuf,
    max_brightness: u32,
    name: String,
}

impl Backlight {
    fn discover() -> Option<Self> {
        let entries = fs::read_dir(SYS_BACKLIGHT).ok()?;

        for entry in entries.flatten() {
            let path = entry.path();
            let brightness = path.join("brightness");
            let max_brightness_path = path.join("max_brightness");
            if brightness.exists()
                && max_brightness_path.exists()
                && let Ok(max) = read_u32(&max_brightness_path)
            {
                if max == 0 {
                    continue;
                }

                let name = entry.file_name().to_string_lossy().to_string();
                return Some(Self {
                    brightness,
                    max_brightness: max,
                    name,
                });
            }
        }

        None
    }

    fn percent(&self) -> io::Result<u8> {
        let raw = read_u32(&self.brightness)?;
        Ok(percent_from_raw(raw, self.max_brightness))
    }

    fn set_percent(&self, percent: u8) -> io::Result<()> {
        let raw = raw_from_percent(percent, self.max_brightness);
        fs::write(&self.brightness, raw.to_string())
    }

    fn adjust_percent(&self, delta: i8) -> io::Result<u8> {
        let current = self.percent()? as i16;
        let next = (current + delta as i16).clamp(0, ABS_MAX_PERCENT as i16) as u8;
        self.set_percent(next)?;
        Ok(next)
    }
}

enum BrightnessCommand {
    Adjust(i8),
}

/// Gauge that reads and adjusts display backlight brightness.
struct BrightnessGauge {
    /// Cached backlight controller; re-discovered when unavailable.
    backlight: Option<Backlight>,
    /// Brightness adjustment delta applied for each scroll/click step.
    step_percent: i8,
    /// Poll cadence for brightness reads and model refresh.
    refresh_interval: Duration,
    /// Sender used by UI callbacks to enqueue brightness adjustments.
    command_tx: mpsc::Sender<BrightnessCommand>,
    /// Receiver drained on each run to apply queued adjustments.
    command_rx: mpsc::Receiver<BrightnessCommand>,
    /// Notifier used to request an immediate scheduler wake-up after actions.
    ready_notify: Option<GaugeReadyNotify>,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for BrightnessGauge {
    fn id(&self) -> &'static str {
        "brightness"
    }

    fn bind_ready_notify(&mut self, notify: GaugeReadyNotify) {
        self.ready_notify = Some(notify);
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<crate::panels::gauges::gauge::GaugeModel> {
        while let Ok(BrightnessCommand::Adjust(delta)) = self.command_rx.try_recv() {
            if self.backlight.is_none() {
                self.backlight = Backlight::discover();
            }
            if let Some(ref ctl) = self.backlight
                && let Err(err) = ctl.adjust_percent(delta)
            {
                log::error!("brightness gauge: failed to adjust brightness: {err}");
                self.backlight = None;
            }
        }

        if self.backlight.is_none() {
            self.backlight = Backlight::discover();
        }

        let percent = if let Some(ref ctl) = self.backlight {
            match ctl.percent() {
                Ok(percent) => Some(percent),
                Err(err) => {
                    log::error!("brightness gauge: failed to read brightness: {err}");
                    self.backlight = None;
                    None
                }
            }
        } else {
            None
        };

        let device_name = self.backlight.as_ref().map(|ctl| ctl.name.clone());
        let step_percent = self.step_percent;
        let command_tx = self.command_tx.clone();
        let ready_notify = self.ready_notify.clone();
        let on_scroll: GaugeClickAction = Arc::new(move |click: GaugeClick| match click.input {
            GaugeInput::ScrollUp => {
                let _ = command_tx.send(BrightnessCommand::Adjust(step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("brightness");
                }
            }
            GaugeInput::ScrollDown => {
                let _ = command_tx.send(BrightnessCommand::Adjust(-step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("brightness");
                }
            }
            _ => {}
        });

        self.next_deadline = now + self.refresh_interval;

        Some(crate::panels::gauges::gauge::GaugeModel {
            id: "brightness",
            icon: svg_asset("brightness.svg"),
            display: brightness_value(percent),
            on_left_click: None,
            on_middle_click: None,
            on_right_click: None,
            on_scroll: Some(on_scroll),
            right_click: None,
            left_click_info: Some(InfoDialog {
                title: "Brightness".to_string(),
                lines: vec![
                    device_name.unwrap_or_else(|| "No backlight device".to_string()),
                    match percent {
                        Some(value) => format!("Brightness: {value}%"),
                        None => "Brightness: N/A".to_string(),
                    },
                ],
            }),
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    let mut step_percent = settings::settings().get_parsed_or(
        "grelier.gauge.brightness.step_percent",
        DEFAULT_STEP_PERCENT,
    );
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let refresh_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.brightness.refresh_interval_secs",
        DEFAULT_REFRESH_INTERVAL_SECS,
    );
    let (command_tx, command_rx) = mpsc::channel::<BrightnessCommand>();
    Box::new(BrightnessGauge {
        backlight: None,
        step_percent,
        refresh_interval: Duration::from_secs(refresh_interval_secs),
        command_tx,
        command_rx,
        ready_notify: None,
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.brightness.step_percent",
            default: "5",
        },
        SettingSpec {
            key: "grelier.gauge.brightness.refresh_interval_secs",
            default: "2",
        },
    ];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "brightness",
        description: "Brightness gauge controlling backlight percent level.",
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
    fn brightness_value_uses_quantity_icon() {
        let GaugeDisplay::Value { value, attention } = brightness_value(Some(50)) else {
            panic!("expected a brightness gauge value");
        };
        let GaugeValue::Svg(handle) = value else {
            panic!("expected svg value for brightness");
        };
        assert_eq!(handle, icon_quantity(50.0 / 100.0));
        assert_eq!(attention, GaugeValueAttention::Nominal);
    }

    #[test]
    fn brightness_value_is_none_on_error() {
        assert!(matches!(brightness_value(None), GaugeDisplay::Error));
    }

    #[test]
    fn percent_conversion_bounds() {
        assert_eq!(percent_from_raw(0, 100), 0);
        assert_eq!(percent_from_raw(50, 100), 50);
        assert_eq!(percent_from_raw(100, 100), 100);
        assert_eq!(percent_from_raw(500, 0), 0);
    }

    #[test]
    fn raw_from_percent_rounds() {
        assert_eq!(raw_from_percent(0, 100), 0);
        assert_eq!(raw_from_percent(50, 100), 50);
        assert_eq!(raw_from_percent(99, 100), 99);
        assert_eq!(raw_from_percent(100, 100), 100);
        assert_eq!(raw_from_percent(50, 3), 2);
    }
}
