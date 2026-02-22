// Test gauge that shows a fixed icon with a cycling quantity value.
use iced::mouse;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{
    ActionSelectAction, GaugeActionDialog, GaugeActionItem, GaugeClick, GaugeClickAction,
    GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention,
};
use crate::panels::gauges::gauge::{Gauge, GaugeReadyNotify};
use crate::panels::gauges::gauge_registry::GaugeSpec;
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

fn action_dialog() -> GaugeActionDialog {
    GaugeActionDialog {
        title: "Test Actions".to_string(),
        items: vec![
            GaugeActionItem {
                id: "ram.svg".to_string(),
                icon: svg_asset("ram.svg"),
            },
            GaugeActionItem {
                id: "disk.svg".to_string(),
                icon: svg_asset("disk.svg"),
            },
            GaugeActionItem {
                id: "microchip.svg".to_string(),
                icon: svg_asset("microchip.svg"),
            },
        ],
        on_select: Some(Arc::new(|item: String| {
            println!("{item}");
        }) as ActionSelectAction),
    }
}

fn info_dialog() -> InfoDialog {
    InfoDialog {
        title: "Test Gauge Info".to_string(),
        lines: vec![
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".to_string(),
            "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string(),
            "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.".to_string(),
        ],
    }
}

struct TestGauge {
    state: Arc<Mutex<QuantityState>>,
    action_dialog: GaugeActionDialog,
    info_dialog: InfoDialog,
    ready_notify: Option<GaugeReadyNotify>,
    next_deadline: Instant,
}

impl Gauge for TestGauge {
    fn id(&self) -> &'static str {
        "test_gauge"
    }

    fn bind_ready_notify(&mut self, notify: GaugeReadyNotify) {
        self.ready_notify = Some(notify);
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let display = self
            .state
            .lock()
            .ok()
            .map(|mut state| state.next())
            .unwrap_or(GaugeDisplay::Error);
        let state = Arc::clone(&self.state);
        let ready_notify = self.ready_notify.clone();
        let on_click: GaugeClickAction = Arc::new(move |click: GaugeClick| {
            if let Ok(mut state) = state.lock()
                && let crate::panels::gauges::gauge::GaugeInput::Button(mouse::Button::Left) =
                    click.input
            {
                state.cycle_attention();
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("test_gauge");
                }
            }
        });
        self.next_deadline = now + Duration::from_secs(1);
        Some(GaugeModel {
            id: "test_gauge",
            icon: svg_asset("option-checked.svg"),
            display,
            on_click: Some(on_click),
            menu: None,
            action_dialog: Some(self.action_dialog.clone()),
            info: Some(self.info_dialog.clone()),
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    Box::new(TestGauge {
        state: Arc::new(Mutex::new(QuantityState::new())),
        action_dialog: action_dialog(),
        info_dialog: info_dialog(),
        ready_notify: None,
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "test_gauge",
        description: "Test gauge emitting canned values for development.",
        default_enabled: false,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn attention_cycles_on_left_click() {
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
    fn info_dialog_attached_to_model() {
        init_settings_once();
        let now = Instant::now();
        let mut gauge = create_gauge(now);
        let first = gauge.run_once(now).expect("gauge model");
        let info = first.info.expect("info dialog should be set");

        assert_eq!(info.title, "Test Gauge Info");
        assert_eq!(info.lines.len(), 3);
    }
}
