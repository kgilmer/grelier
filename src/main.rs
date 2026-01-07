#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod gauges {
    pub mod battery;
    pub mod brightness;
    pub mod clock;
    pub mod cpu;
    pub mod date;
    pub mod disk;
    pub mod audio_in;
    pub mod audio_out;
    pub mod net_common;
    pub mod net_down;
    pub mod net_up;
    pub mod test_gauge;
    pub mod ram;
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

use crate::app::Orientation;
use crate::app::{BarState, Message};
use crate::gauge::{GaugeClick, GaugeModel};
use crate::gauges::{
    audio_in, audio_out, battery, brightness, clock, cpu, date, disk, net_down, net_up, ram,
    test_gauge,
};

#[derive(FromArgs, Debug)]
/// Workspace + gauges display
struct Args {
    /// clock, date, battery, cpu, disk, ram, net_up, net_down, audio_out, audio_in, brightness
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
            "cpu" => subs.push(cpu::cpu_subscription()),
            "disk" => subs.push(disk::disk_subscription()),
            "net_down" => subs.push(net_down::net_down_subscription()),
            "net_up" => subs.push(net_up::net_up_subscription()),
            "ram" => subs.push(ram::ram_subscription()),
            "test_gauge" => subs.push(test_gauge::test_gauge_subscription()), // Special test gauge, intentionally omitted from help text
            "audio_out" => subs.push(audio_out::audio_out_subscription()),
            "brightness" => subs.push(brightness::brightness_subscription()),
            "audio_in" => subs.push(audio_in::audio_in_subscription()),
            other => unreachable!("gauges validated before subscription: {other}"),
        }
    }
    Subscription::batch(subs)
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    const KNOWN_GAUGES: &[&str] = &[
        "clock",
        "date",
        "battery",
        "cpu",
        "disk",
        "ram",
        "net_up",
        "net_down",
        "audio_out",
        "audio_in",
        "brightness",
        "test_gauge",
    ];

    let gauges: Vec<String> = args
        .gauges
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    for gauge in &gauges {
        if !KNOWN_GAUGES.contains(&gauge.as_str()) {
            eprintln!("Unknown gauge '{gauge}'. Known gauges: {}", KNOWN_GAUGES.join(", "));
            std::process::exit(1);
        }
    }

    let anchor = match args.orientation {
        Orientation::Left => Anchor::Left,
        Orientation::Right => Anchor::Right,
    };

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some((28, 0)),
            exclusive_zone: 28,
            anchor,
            layer: Layer::Top,
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

    let gauge_order = gauges.clone();

    application(
        move || BarState::with_gauge_order(gauge_order.clone()),
        BarState::namespace,
        update,
        BarState::view,
    )
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
        Message::WorkspaceClicked(name) => {
            if let Err(err) = sway_workspace::focus_workspace(&name) {
                eprintln!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::Gauge(gauge) => {
            update_gauge(&mut state.gauges, gauge);
        }
        Message::GaugeClicked { id, target, input } => {
            if let Some(callback) = state
                .gauges
                .iter()
                .find(|g| g.id == id)
                .and_then(|g| g.on_click.clone())
            {
                callback(GaugeClick { input, target });
            } else {
                println!("Gauge '{id}' clicked: {:?} {:?}", target, input);
            }
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
            value: Some(GaugeValue::Text("12\n00".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
        };
        let g2 = GaugeModel {
            id: "clock",
            icon: None,
            value: Some(GaugeValue::Text("12\n01".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
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
            value: Some(GaugeValue::Text("01\n01".to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
        };
        update_gauge(&mut gauges, g3.clone());
        assert_eq!(gauges.len(), 2, "different id should append");
    }

    fn assert_text_value(model: &GaugeModel, expected: &str) {
        match &model.value {
            Some(GaugeValue::Text(text)) => assert_eq!(text, expected),
            Some(GaugeValue::Svg(_)) => panic!("expected text gauge value"),
            None => panic!("expected value"),
        }
    }
}
