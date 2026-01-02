#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod gauges {
    pub mod battery;
    pub mod clock;
    pub mod date;
}
mod gauge;
mod sway_workspace;

use std::path::Path;
use std::sync::Arc;

use argh::FromArgs;
use iced::Color;
use iced::Subscription;
use iced::Task;
use iced::Theme;
use iced::futures::{StreamExt, channel::mpsc};
use iced::theme::Custom;
use iced::theme::Palette;
use iced::widget::svg;
use iced_layershell::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use swayipc::Event;

use crate::app::{BarState, Message, WorkspaceInfo};
use crate::gauge::GaugeModel;
use crate::gauges::{battery, clock, date};

/// Absolute path to the bundled asset directory (e.g. SVG icons).
pub const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
pub const DEFAULT_THEME: Theme = Theme::Nord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Left,
    Right,
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation::Left
    }
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
    /// comma-separated list of gauges to run (clock,date,...)
    #[argh(option, default = "\"clock,date\".to_string()")]
    gauges: String,

    /// list all available gauges and exit
    #[argh(switch)]
    list_gauges: bool,

    /// orientation of the bar (left or right)
    #[argh(option, default = "Orientation::Left")]
    orientation: Orientation,

    /// theme name: CatppuccinFrappe,CatppuccinLatte,CatppuccinMacchiato,CatppuccinMocha,Dark,Dracula,Ferra,GruvboxDark,GruvboxLight,KanagawaDragon,KanagawaLotus,KanagawaWave,Light,Moonfly,Nightfly,Nord,Oxocarbon,TokyoNight,TokyoNightLight,TokyoNightStorm,AyuMirage
    #[argh(option)]
    theme: Option<String>,
}

fn parse_theme(name: &str) -> Option<Theme> {
    match name {
        "CatppuccinFrappe" => Some(Theme::CatppuccinFrappe),
        "CatppuccinLatte" => Some(Theme::CatppuccinLatte),
        "CatppuccinMacchiato" => Some(Theme::CatppuccinMacchiato),
        "CatppuccinMocha" => Some(Theme::CatppuccinMocha),
        "Dark" => Some(Theme::Dark),
        "Dracula" => Some(Theme::Dracula),
        "Ferra" => Some(Theme::Ferra),
        "GruvboxDark" => Some(Theme::GruvboxDark),
        "GruvboxLight" => Some(Theme::GruvboxLight),
        "KanagawaDragon" => Some(Theme::KanagawaDragon),
        "KanagawaLotus" => Some(Theme::KanagawaLotus),
        "KanagawaWave" => Some(Theme::KanagawaWave),
        "Light" => Some(Theme::Light),
        "Moonfly" => Some(Theme::Moonfly),
        "Nightfly" => Some(Theme::Nightfly),
        "Nord" => Some(Theme::Nord),
        "Oxocarbon" => Some(Theme::Oxocarbon),
        "TokyoNight" => Some(Theme::TokyoNight),
        "TokyoNightLight" => Some(Theme::TokyoNightLight),
        "TokyoNightStorm" => Some(Theme::TokyoNightStorm),
        "AyuMirage" => Some(Theme::Custom(Arc::new(Custom::new(
            "AyuMirage".to_string(),
            Palette {
                background: Color::from_rgb8(0x1F, 0x24, 0x30),
                text: Color::from_rgb8(0xCB, 0xCC, 0xC6),
                primary: Color::from_rgb8(0xFF, 0xCC, 0x66),
                success: Color::from_rgb8(0xBA, 0xE6, 0x7E),
                warning: Color::from_rgb8(0xFF, 0xD1, 0x73),
                danger: Color::from_rgb8(0xF2, 0x87, 0x79),
            },
        )))),
        _ => None,
    }
}

/// Build an `iced` SVG handle for a file under `assets/`.
pub fn svg_asset(name: &str) -> svg::Handle {
    let path = Path::new(ASSETS_DIR).join(name);
    svg::Handle::from_path(path)
}

fn app_subscription(_state: &BarState, gauges: &[&str]) -> Subscription<Message> {
    let mut subs = Vec::new();
    subs.push(workspace_subscription());
    for gauge in gauges {
        match *gauge {
            "clock" => subs.push(clock_subscription()),
            "date" => subs.push(date_subscription()),
            "battery" => subs.push(battery_subscription()),
            other => eprintln!("Unknown gauge '{other}', skipping"),
        }
    }
    Subscription::batch(subs)
}

fn workspace_subscription() -> Subscription<Message> {
    Subscription::run(workspace_stream)
}

fn workspace_stream() -> impl iced::futures::Stream<Item = Message> {
    let (mut sender, receiver) = mpsc::channel(16);

    std::thread::spawn(move || {
        let send_workspaces =
            |sender: &mut mpsc::Sender<Message>| match sway_workspace::fetch_workspaces() {
                Ok(ws) => {
                    let info = ws.into_iter().map(to_workspace_info).collect();
                    let _ = sender.try_send(Message::Workspaces(info));
                }
                Err(err) => eprintln!("Failed to fetch workspaces: {err}"),
            };

        send_workspaces(&mut sender);

        let mut stream = match sway_workspace::subscribe_workspace_events() {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("Failed to subscribe to workspace events: {err}");
                return;
            }
        };

        for event in &mut stream {
            match event {
                Ok(Event::Workspace(_)) => send_workspaces(&mut sender),
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Workspace event stream error: {err}");
                    break;
                }
            }
        }
    });

    receiver
}

fn clock_subscription() -> Subscription<Message> {
    Subscription::run(|| clock::seconds_stream().map(Message::Gauge))
}

fn date_subscription() -> Subscription<Message> {
    Subscription::run(|| date::day_stream().map(Message::Gauge))
}

fn battery_subscription() -> Subscription<Message> {
    Subscription::run(|| battery::battery_stream().map(Message::Gauge))
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    if args.list_gauges {
        println!("Available gauges: clock, date, battery");
        return Ok(());
    }

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
            size: Some((24, 0)), // width fixed to 24px, height chosen by compositor (anchored top+bottom)
            exclusive_zone: 24,
            anchor,
            layer: Layer::Overlay,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode: StartMode::Active,
            events_transparent: false,
        },
        antialiasing: true,
        ..Settings::default()
    };

    let theme = args
        .theme
        .as_deref()
        .and_then(parse_theme)
        .unwrap_or(DEFAULT_THEME);

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
    use super::*;

    #[test]
    fn update_gauge_replaces_by_id() {
        let mut gauges = Vec::new();
        let g1 = GaugeModel {
            id: "clock",
            icon: None,
            value: "12\n00".to_string(),
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            value: "12\n01".to_string(),
        };

        update_gauge(&mut gauges, g1.clone());
        assert_eq!(gauges.len(), 1);
        assert_eq!(gauges[0].value, g1.value);

        update_gauge(&mut gauges, g2.clone());
        assert_eq!(gauges.len(), 1, "should replace existing entry");
        assert_eq!(gauges[0].value, g2.value);

        let g3 = GaugeModel {
            id: "date",
            icon: None,
            value: "01\n01".to_string(),
        };
        update_gauge(&mut gauges, g3.clone());
        assert_eq!(gauges.len(), 2, "different id should append");
    }
}

fn to_workspace_info(ws: swayipc::Workspace) -> WorkspaceInfo {
    let rect = crate::app::Rect {
        x: ws.rect.x,
        y: ws.rect.y,
        width: ws.rect.width,
        height: ws.rect.height,
    };

    WorkspaceInfo {
        id: ws.id,
        num: ws.num,
        name: ws.name,
        layout: ws.layout,
        visible: ws.visible,
        focused: ws.focused,
        urgent: ws.urgent,
        representation: ws.representation,
        orientation: ws.orientation,
        rect,
        output: ws.output,
        focus: ws.focus,
    }
}
