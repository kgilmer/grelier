// Test gauge that cycles quantity icons and toggles style/attention on clicks.
// Consumes Settings: grelier.gauge.test_gauge.quantitystyle.
use iced::mouse;
use std::sync::Mutex;
use std::time::Duration;

use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval,
};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::icon::{QuantityStyle, icon_quantity};
use crate::settings;
use std::sync::Arc;

// Step sized to traverse all grid icons (0-9) without skipping.
const STEP: f32 = 1.0 / 9.0;

#[derive(Debug, Clone, Copy)]
enum QuantityMode {
    Grid,
    Pie,
}

/// Tracks a ping-pong sequence over the pie icon indices.
#[derive(Debug)]
struct BounceSequence {
    value: f32,
    descending: bool,
}

impl BounceSequence {
    fn new() -> Self {
        Self {
            value: 0.0,
            descending: false,
        }
    }

    /// Return the current value and advance, bouncing at both ends.
    fn next(&mut self) -> f32 {
        let current = self.value;
        if self.descending {
            let next = (self.value - STEP).max(0.0);
            self.value = next;
            if next <= 0.0 {
                self.descending = false;
            }
        } else {
            let next = (self.value + STEP).min(1.0);
            self.value = next;
            if next >= 1.0 {
                self.descending = true;
            }
        }
        current
    }
}

#[derive(Debug)]
struct QuantityState {
    sequence: BounceSequence,
    mode: QuantityMode,
    attention: GaugeValueAttention,
}

impl QuantityState {
    fn new(style: QuantityStyle) -> Self {
        Self {
            sequence: BounceSequence::new(),
            mode: match style {
                QuantityStyle::Grid => QuantityMode::Grid,
                QuantityStyle::Pie => QuantityMode::Pie,
            },
            attention: GaugeValueAttention::Nominal,
        }
    }

    fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            QuantityMode::Grid => QuantityMode::Pie,
            QuantityMode::Pie => QuantityMode::Grid,
        };
    }

    fn cycle_attention(&mut self) {
        self.attention = match self.attention {
            GaugeValueAttention::Nominal => GaugeValueAttention::Warning,
            GaugeValueAttention::Warning => GaugeValueAttention::Danger,
            GaugeValueAttention::Danger => GaugeValueAttention::Nominal,
        };
    }

    fn next(&mut self) -> (Option<GaugeValue>, GaugeValueAttention) {
        match self.mode {
            QuantityMode::Grid => (
                Some(GaugeValue::Svg(icon_quantity(
                    QuantityStyle::Grid,
                    self.sequence.next(),
                ))),
                self.attention,
            ),
            QuantityMode::Pie => (
                Some(GaugeValue::Svg(icon_quantity(
                    QuantityStyle::Pie,
                    self.sequence.next(),
                ))),
                self.attention,
            ),
        }
    }
}

/// Cycles over pie-[0-8].svg, bouncing when hitting the ends.
fn test_gauge_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let style_value = settings::settings().get_or("grelier.gauge.test_gauge.quantitystyle", "pie");
    let style =
        QuantityStyle::parse_setting("grelier.gauge.test_gauge.quantitystyle", &style_value);
    let state = Arc::new(Mutex::new(QuantityState::new(style)));
    let on_click: GaugeClickAction = {
        let state = Arc::clone(&state);
        Arc::new(move |click: GaugeClick| {
            let (_mode, _attention) = if let Ok(mut state) = state.lock() {
                match click.input {
                    crate::gauge::GaugeInput::Button(mouse::Button::Right) => {
                        state.cycle_attention()
                    }
                    crate::gauge::GaugeInput::Button(mouse::Button::Left) => {
                        state.cycle_mode();
                        let style_value = match state.mode {
                            QuantityMode::Grid => QuantityStyle::Grid,
                            QuantityMode::Pie => QuantityStyle::Pie,
                        };
                        settings::settings().update(
                            "grelier.gauge.test_gauge.quantitystyle",
                            style_value.as_setting_value(),
                        );
                    }
                    _ => {}
                }
                (state.mode, state.attention)
            } else {
                (QuantityMode::Grid, GaugeValueAttention::Nominal)
            };
        })
    };

    fixed_interval(
        "test_gauge",
        None,
        || Duration::from_secs(1),
        {
            let state = Arc::clone(&state);
            move || {
                let mut state = state.lock().ok()?;
                let (value, attention) = state.next();
                Some((value, attention))
            }
        },
        Some(on_click),
    )
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[SettingSpec {
        key: "grelier.gauge.test_gauge.quantitystyle",
        default: "pie",
    }];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(test_gauge_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "test_gauge",
        label: "Test Gauge",
        description: "Test gauge emitting canned values for development.",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pie_sequence_bounces() {
        let mut seq = BounceSequence::new();
        let produced: Vec<_> = (0..10).map(|_| seq.next()).collect();
        assert_eq!(
            produced,
            vec![
                0.0,
                1.0 / 9.0,
                2.0 / 9.0,
                3.0 / 9.0,
                4.0 / 9.0,
                5.0 / 9.0,
                6.0 / 9.0,
                7.0 / 9.0,
                8.0 / 9.0,
                1.0
            ]
        );
    }

    #[test]
    fn attention_cycles_on_right_click() {
        let mut state = QuantityState::new(QuantityStyle::Pie);
        assert_eq!(state.attention, GaugeValueAttention::Nominal);

        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Warning);
        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Danger);
        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Nominal);
    }

    #[test]
    fn mode_cycles_between_styles() {
        let mut state = QuantityState::new(QuantityStyle::Grid);

        assert!(matches!(state.mode, QuantityMode::Grid));
        state.cycle_mode();
        assert!(matches!(state.mode, QuantityMode::Pie));
        state.cycle_mode();
        assert!(matches!(state.mode, QuantityMode::Grid));
    }
}
