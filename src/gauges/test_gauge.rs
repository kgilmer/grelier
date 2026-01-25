// Test gauge that cycles quantity icons and toggles attention on clicks.
use iced::futures::StreamExt;
use iced::mouse;
use std::sync::Mutex;
use std::time::Duration;

use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval,
};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::icon::icon_quantity;
use crate::info_dialog::InfoDialog;
use std::sync::Arc;

// Step sized to traverse the full range without skipping endpoints.
const STEP: f32 = 1.0 / 9.0;

/// Tracks a ping-pong sequence over the icon indices.
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

    fn next(&mut self) -> (Option<GaugeValue>, GaugeValueAttention) {
        (
            Some(GaugeValue::Svg(icon_quantity(self.sequence.next()))),
            self.attention,
        )
    }
}

/// Cycles over the available quantity icons, bouncing when hitting the ends.
fn test_gauge_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
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
                match click.input {
                    crate::gauge::GaugeInput::Button(mouse::Button::Right) => {
                        state.cycle_attention()
                    }
                    _ => {}
                }
                state.attention
            } else {
                GaugeValueAttention::Nominal
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
    use iced::futures::executor::block_on;
    use std::sync::Once;

    use crate::settings_storage::SettingsStorage;

    fn init_settings_once() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let mut path = std::env::temp_dir();
            path.push("grelier_test_gauge_settings");
            path.push("Settings.xresources");
            let storage = SettingsStorage::new(path);
            let settings = crate::settings::Settings::new(storage);
            let _ = crate::settings::init_settings(settings);
        });
    }

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
