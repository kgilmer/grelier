use crate::app::Message;
use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeInput, GaugeValue, GaugeValueAttention, SettingSpec,
    event_stream,
};
use crate::icon::svg_asset;
use crate::settings;
use iced::Subscription;
use iced::futures::StreamExt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

const DEFAULT_STEP_PERCENT: i8 = 5;
const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 2;
const DISPLAY_MAX: u8 = 99;
const ABS_MAX_PERCENT: u8 = 100;
const SYS_BACKLIGHT: &str = "/sys/class/backlight";

fn format_percent(value: u8) -> String {
    format!("{:02}", value.min(DISPLAY_MAX))
}

fn brightness_value(percent: Option<u8>) -> (Option<GaugeValue>, GaugeValueAttention) {
    match percent {
        Some(p) => (
            Some(GaugeValue::Text(format_percent(p))),
            GaugeValueAttention::Nominal,
        ),
        None => (None, GaugeValueAttention::Danger),
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

                return Some(Self {
                    brightness,
                    max_brightness: max,
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

fn brightness_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let (command_tx, command_rx) = mpsc::channel::<BrightnessCommand>();
    let mut step_percent =
        settings::settings().get_parsed_or("grelier.brightness.step_percent", DEFAULT_STEP_PERCENT);
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let refresh_interval_secs = settings::settings().get_parsed_or(
        "grelier.brightness.refresh_interval_secs",
        DEFAULT_REFRESH_INTERVAL_SECS,
    );

    let on_click: GaugeClickAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |click: GaugeClick| match click.input {
            GaugeInput::ScrollUp => {
                let _ = command_tx.send(BrightnessCommand::Adjust(step_percent));
            }
            GaugeInput::ScrollDown => {
                let _ = command_tx.send(BrightnessCommand::Adjust(-step_percent));
            }
            _ => {}
        })
    };

    event_stream(
        "brightness",
        Some(svg_asset("brightness.svg")),
        move |mut sender| {
            let mut backlight = Backlight::discover();

            let mut send_state = |percent: Option<u8>, attention: GaugeValueAttention| {
                let (value, default_attention) = brightness_value(percent);
                let attention = if value.is_some() {
                    attention
                } else {
                    default_attention
                };

                let _ = sender.try_send(crate::gauge::GaugeModel {
                    id: "brightness",
                    icon: Some(svg_asset("brightness.svg")),
                    value,
                    attention,
                    on_click: Some(on_click.clone()),
                    menu: None,
                });
            };

            let mut percent = None;
            if let Some(ref ctl) = backlight {
                match ctl.percent() {
                    Ok(p) => percent = Some(p),
                    Err(err) => {
                        eprintln!("brightness gauge: failed to read brightness: {err}");
                        backlight = None;
                    }
                }
            }

            let attention = if percent.is_some() {
                GaugeValueAttention::Nominal
            } else {
                GaugeValueAttention::Danger
            };

            send_state(percent, attention);

            loop {
                let needs_refresh =
                    match command_rx.recv_timeout(Duration::from_secs(refresh_interval_secs)) {
                        Ok(BrightnessCommand::Adjust(delta)) => {
                            if backlight.is_none() {
                                backlight = Backlight::discover();
                            }

                            if let Some(ref ctl) = backlight
                                && let Err(err) = ctl.adjust_percent(delta)
                            {
                                eprintln!("brightness gauge: failed to adjust brightness: {err}");
                                backlight = None;
                            }
                            true
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if backlight.is_none() {
                                backlight = Backlight::discover();
                            }
                            true
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    };

                if needs_refresh {
                    let percent = if let Some(ref ctl) = backlight {
                        match ctl.percent() {
                            Ok(p) => Some(p),
                            Err(err) => {
                                eprintln!("brightness gauge: failed to read brightness: {err}");
                                backlight = None;
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let attention = if percent.is_some() {
                        GaugeValueAttention::Nominal
                    } else {
                        GaugeValueAttention::Danger
                    };

                    send_state(percent, attention);
                }
            }
        },
    )
}

pub fn brightness_subscription() -> Subscription<Message> {
    Subscription::run(|| brightness_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.brightness.step_percent",
            default: "5",
        },
        SettingSpec {
            key: "grelier.brightness.refresh_interval_secs",
            default: "2",
        },
    ];
    SETTINGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_to_two_digits() {
        assert_eq!(format_percent(0), "00");
        assert_eq!(format_percent(7), "07");
        assert_eq!(format_percent(99), "99");
        assert_eq!(format_percent(120), "99");
    }

    #[test]
    fn brightness_value_is_none_on_error() {
        let (value, attention) = brightness_value(None);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
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
