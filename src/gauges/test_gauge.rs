use iced::{Subscription, mouse};
use std::sync::Mutex;
use std::time::Duration;

use crate::app::Message;
use crate::gauge::{GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention, fixed_interval};
use crate::icon::{QuantityStyle, icon_quantity};
use iced::futures::StreamExt;
use std::sync::Arc;

// Step sized to traverse all grid icons (0-9) without skipping.
const STEP: f32 = 1.0 / 9.0;

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
    style: QuantityStyle,
    attention: GaugeValueAttention,
}

impl QuantityState {
    fn new(style: QuantityStyle) -> Self {
        Self {
            sequence: BounceSequence::new(),
            style,
            attention: GaugeValueAttention::Nominal,
        }
    }

    fn toggle_style(&mut self) {
        self.style = match self.style {
            QuantityStyle::Grid => QuantityStyle::Pie,
            QuantityStyle::Pie => QuantityStyle::Grid,
        };
    }

    fn cycle_attention(&mut self) {
        self.attention = match self.attention {
            GaugeValueAttention::Nominal => GaugeValueAttention::Warning,
            GaugeValueAttention::Warning => GaugeValueAttention::Danger,
            GaugeValueAttention::Danger => GaugeValueAttention::Nominal,
        };
    }

    fn next(&mut self) -> (QuantityStyle, f32, GaugeValueAttention) {
        (self.style, self.sequence.next(), self.attention)
    }
}

/// Cycles over pie-[0-8].svg, bouncing when hitting the ends.
fn test_gauge_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let state = Arc::new(Mutex::new(QuantityState::new(QuantityStyle::Pie)));
    let on_click: GaugeClickAction = {
        let state = Arc::clone(&state);
        Arc::new(move |click: GaugeClick| {
            let (_style, _attention) = if let Ok(mut state) = state.lock() {
                match click.input {
                    crate::gauge::GaugeInput::Button(mouse::Button::Right) => {
                        state.cycle_attention()
                    }
                    crate::gauge::GaugeInput::Button(_) => state.toggle_style(),
                    _ => {}
                }
                (state.style, state.attention)
            } else {
                (QuantityStyle::Grid, GaugeValueAttention::Nominal)
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
                let (style, value, attention) = state.next();
                println!("rendering style {:?} for {}", style, value);
                Some((GaugeValue::Svg(icon_quantity(style, value)), attention))
            }
        },
        Some(on_click),
    )
}

pub fn test_gauge_subscription() -> Subscription<Message> {
    Subscription::run(|| test_gauge_stream().map(Message::Gauge))
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
}
