#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod gauges {
    pub mod battery;
    pub mod clock;
    pub mod date;
}
mod gauge;
mod icon;
mod sway_workspace;
mod theme;

use argh::FromArgs;
use iced::Font;
use iced::Subscription;
use iced::Task;

use iced_layershell::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};

use crate::app::{BarState, Message};
use crate::gauge::GaugeModel;
use crate::gauges::{battery, clock, date};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Orientation {
    #[default]
    Left,
    Right,
}

impl std::str::FromStr for Orientation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "left" => Ok(Orientation::Left),
            "right" => Ok(Orientation::Right),
            other => Err(format!(
                "Invalid orientation '{other}', expected 'left' or 'right'"
            )),
        }
    }
}

#[derive(FromArgs, Debug)]
/// Workspace + gauges display
struct Args {
    /// gauges: clock, date, battery
    #[argh(option, default = "\"clock,date\".to_string()")]
    gauges: String,

    /// orientation of the bar (left or right)
    #[argh(option, default = "Orientation::Left")]
    orientation: Orientation,

    /// theme name: CatppuccinFrappe,CatppuccinLatte,CatppuccinMacchiato,CatppuccinMocha,Dark,Dracula,Ferra,GruvboxDark,GruvboxLight,KanagawaDragon,KanagawaLotus,KanagawaWave,Light,Moonfly,Nightfly,Nord,Oxocarbon,TokyoNight,TokyoNightLight,TokyoNightStorm,AyuMirage
    #[argh(option)]
    theme: Option<String>,
}

fn app_subscription(_state: &BarState, gauges: &[&str]) -> Subscription<Message> {
    let mut subs = Vec::new();
    subs.push(sway_workspace::workspace_subscription());
    for gauge in gauges {
        match *gauge {
            "clock" => subs.push(clock::clock_subscription()),
            "date" => subs.push(date::date_subscription()),
            "battery" => subs.push(battery::battery_subscription()),
            other => eprintln!("Unknown gauge '{other}', skipping"),
        }
    }
    Subscription::batch(subs)
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    let gauges: Vec<String> = args
        .gauges
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let anchor = match args.orientation {
        Orientation::Left => Anchor::Top | Anchor::Bottom | Anchor::Left,
        Orientation::Right => Anchor::Top | Anchor::Bottom | Anchor::Right,
    };

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some((28, 0)),
            exclusive_zone: 28,
            anchor,
            layer: Layer::Overlay,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode: StartMode::Active,
            events_transparent: false,
        },
        antialiasing: true,
        default_font: Font::MONOSPACE,
        ..Settings::default()
    };

    let theme = args
        .theme
        .as_deref()
        .and_then(theme::parse_them)
        .unwrap_or(theme::DEFAULT_THEME);

    application(BarState::new, BarState::namespace, update, BarState::view)
        .theme(theme)
        .subscription({
            let gauges = gauges.clone();
            move |state| {
                let gauge_refs: Vec<&str> = gauges.iter().map(|s| s.as_str()).collect();
                app_subscription(state, &gauge_refs)
            }
        })
        .settings(settings)
        .run()
}

fn update(state: &mut BarState, message: Message) -> Task<Message> {
    match message {
        Message::Workspaces(ws) => state.workspaces = ws,
        Message::Clicked(name) => {
            if let Err(err) = sway_workspace::focus_workspace(&name) {
                eprintln!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::Gauge(gauge) => {
            update_gauge(&mut state.gauges, gauge);
        }
    }

    Task::none()
}

fn update_gauge(gauges: &mut Vec<GaugeModel>, new: GaugeModel) {
    if let Some(existing) = gauges.iter_mut().find(|g| g.id == new.id) {
        *existing = new;
    } else {
        gauges.push(new);
    }
}

#[cfg(test)]
mod tests {
    use crate::gauge::{GaugeValue, GaugeValueAttention};

    use super::*;

    #[test]
    fn update_gauge_replaces_by_id() {
        let mut gauges = Vec::new();
        let g1 = GaugeModel {
            id: "clock",
            icon: None,
            value: GaugeValue::Text("12\n00".to_string()),
            attention: GaugeValueAttention::Nominal,
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            value: GaugeValue::Text("12\n01".to_string()),
            attention: GaugeValueAttention::Nominal,
        };

        update_gauge(&mut gauges, g1.clone());
        assert_eq!(gauges.len(), 1);
        assert_text_value(&gauges[0], "12\n00");

        update_gauge(&mut gauges, g2.clone());
        assert_eq!(gauges.len(), 1, "should replace existing entry");
        assert_text_value(&gauges[0], "12\n01");

        let g3 = GaugeModel {
            id: "date",
            icon: None,
            value: GaugeValue::Text("01\n01".to_string()),
            attention: GaugeValueAttention::Nominal,
        };
        update_gauge(&mut gauges, g3.clone());
        assert_eq!(gauges.len(), 2, "different id should append");
    }

    fn assert_text_value(model: &GaugeModel, expected: &str) {
        match &model.value {
            GaugeValue::Text(text) => assert_eq!(text, expected),
            GaugeValue::Svg(_) => panic!("expected text gauge value"),
        }
    }
}
