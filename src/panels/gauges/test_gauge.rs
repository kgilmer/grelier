// Test gauge that shows a fixed icon with a cycling quantity value.
use iced::futures::StreamExt;
use iced::mouse;
use std::sync::Mutex;
use std::time::Duration;

use crate::icon::{icon_quantity, svg_asset};
use crate::info_dialog::InfoDialog;
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeValue, GaugeValueAttention, fixed_interval,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings::SettingSpec;
use std::sync::Arc;

// Step sized to traverse the full range without skipping endpoints.
const STEP: f32 = 1.0 / 9.0;

/// Tracks a ping-pong sequence over the icon indices.
#[derive(Debug)]
struct BounceSequence {
    value: f32,
    descending: bool,
    emit_none_at_top: bool,
}

impl BounceSequence {
    fn new() -> Self {
        Self {
            value: 0.0,
            descending: false,
            emit_none_at_top: false,
        }
    }

    /// Return the current value and advance, bouncing at both ends.
    fn next(&mut self) -> Option<f32> {
        if self.emit_none_at_top {
            self.emit_none_at_top = false;
            return None;
        }

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
                self.emit_none_at_top = true;
                self.descending = true;
            }
        }
        Some(current)
    }
}

#[derive(Debug)]
struct QuantityState {
    sequence: BounceSequence,
    attention: GaugeValueAttention,
}

impl QuantityState {
    fn new() -> Self {
        Self {
            sequence: BounceSequence::new(),
            attention: GaugeValueAttention::Nominal,
        }
    }

    fn cycle_attention(&mut self) {
        self.attention = match self.attention {
            GaugeValueAttention::Nominal => GaugeValueAttention::Warning,
            GaugeValueAttention::Warning => GaugeValueAttention::Danger,
            GaugeValueAttention::Danger => GaugeValueAttention::Nominal,
        };
    }

    fn next(&mut self) -> GaugeDisplay {
        let value = self.sequence.next();
        match value {
            Some(value) => GaugeDisplay::Value {
                value: GaugeValue::Svg(icon_quantity(value)),
                attention: self.attention,
            },
            None => GaugeDisplay::Error,
        }
    }
}

/// Emits a steady icon with a cycling quantity value and updates attention on clicks.
fn test_gauge_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel>
{
    let state = Arc::new(Mutex::new(QuantityState::new()));
    let info_dialog = InfoDialog {
        title: "Test Gauge Info".to_string(),
        lines: vec![
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".to_string(),
            "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string(),
            "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.".to_string(),
        ],
    };
    let on_click: GaugeClickAction = {
        let state = Arc::clone(&state);
        Arc::new(move |click: GaugeClick| {
            let _attention = if let Ok(mut state) = state.lock() {
                if let crate::panels::gauges::gauge::GaugeInput::Button(mouse::Button::Right) =
                    click.input
                {
                    state.cycle_attention();
                }
                state.attention
            } else {
                GaugeValueAttention::Nominal
            };
        })
    };

    fixed_interval(
        "test_gauge",
        Some(svg_asset("option-checked.svg")),
        || Duration::from_secs(1),
        {
            let state = Arc::clone(&state);
            move || {
                let mut state = state.lock().ok()?;
                Some(state.next())
            }
        },
        Some(on_click),
    )
    .map({
        let info_dialog = info_dialog.clone();
        move |mut model| {
            model.info = Some(info_dialog.clone());
            model
        }
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(test_gauge_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "test_gauge",
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
    use iced::futures::executor::block_on;
    use std::sync::Once;

    use crate::settings_storage::SettingsStorage;

    fn init_settings_once() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let mut path = std::env::temp_dir();
            path.push("grelier_test_gauge_settings");
            path.push(format!("Settings-{}.xresources", env!("CARGO_PKG_VERSION")));
            let storage = SettingsStorage::new(path);
            let settings = crate::settings::Settings::new(storage);
            let _ = crate::settings::init_settings(settings);
        });
    }

    #[test]
    fn quantity_sequence_bounces() {
        let mut seq = BounceSequence::new();
        let produced: Vec<_> = (0..10).map(|_| seq.next()).collect();
        assert_eq!(
            produced,
            vec![
                Some(0.0),
                Some(1.0 / 9.0),
                Some(2.0 / 9.0),
                Some(3.0 / 9.0),
                Some(4.0 / 9.0),
                Some(5.0 / 9.0),
                Some(6.0 / 9.0),
                Some(7.0 / 9.0),
                Some(8.0 / 9.0),
                None
            ]
        );
    }

    #[test]
    fn attention_cycles_on_right_click() {
        let mut state = QuantityState::new();
        assert_eq!(state.attention, GaugeValueAttention::Nominal);

        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Warning);
        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Danger);
        state.cycle_attention();
        assert_eq!(state.attention, GaugeValueAttention::Nominal);
    }

    #[test]
    fn info_dialog_attached_to_stream() {
        init_settings_once();
        let mut stream = test_gauge_stream();
        let first = block_on(stream.next()).expect("gauge model");
        let info = first.info.expect("info dialog should be set");

        assert_eq!(info.title, "Test Gauge Info");
        assert_eq!(info.lines.len(), 3);
    }
}
