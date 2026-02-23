use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use grelier::bar::{BarState, Message};
use grelier::gauges::gauge::{GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention};
use grelier::runtime_dispatch;
use grelier::sway_workspace::{Rect, WorkspaceApps, WorkspaceInfo};

use iced::{Event, Point, mouse};

#[derive(Clone, Copy)]
struct DispatchScenario {
    name: &'static str,
    rounds: usize,
}

fn synthetic_gauge_model(id: &'static str, text: &str) -> GaugeModel {
    GaugeModel {
        id,
        icon: None,
        display: GaugeDisplay::Value {
            value: GaugeValue::Text(text.to_string()),
            attention: GaugeValueAttention::Nominal,
        },
        on_click: None,
        menu: None,
        action_dialog: None,
        info: None,
    }
}

fn synthetic_gauge_batch() -> Vec<GaugeModel> {
    vec![
        synthetic_gauge_model("cpu", "15%"),
        synthetic_gauge_model("ram", "41%"),
        synthetic_gauge_model("net_down", "120K"),
        synthetic_gauge_model("net_up", "18K"),
        synthetic_gauge_model("clock", "10\n42"),
    ]
}

fn synthetic_workspaces() -> Message {
    Message::Workspaces {
        workspaces: vec![
            WorkspaceInfo {
                num: 1,
                name: "1:web".to_string(),
                focused: true,
                urgent: false,
                rect: Rect { y: 0, height: 1080 },
            },
            WorkspaceInfo {
                num: 2,
                name: "2:code".to_string(),
                focused: false,
                urgent: false,
                rect: Rect { y: 0, height: 1080 },
            },
            WorkspaceInfo {
                num: 3,
                name: "3:term".to_string(),
                focused: false,
                urgent: true,
                rect: Rect { y: 0, height: 1080 },
            },
        ],
        apps: vec![
            WorkspaceApps {
                name: "1:web".to_string(),
                apps: vec![],
            },
            WorkspaceApps {
                name: "2:code".to_string(),
                apps: vec![],
            },
            WorkspaceApps {
                name: "3:term".to_string(),
                apps: vec![],
            },
        ],
    }
}

fn representative_mix() -> Vec<Message> {
    vec![
        Message::GaugeBatch(synthetic_gauge_batch()),
        Message::IcedEvent(Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(8.0, 14.0),
        })),
        Message::GaugeBatch(synthetic_gauge_batch()),
        synthetic_workspaces(),
        Message::GaugeBatch(synthetic_gauge_batch()),
        Message::IcedEvent(Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(11.0, 80.0),
        })),
        Message::GaugeBatch(synthetic_gauge_batch()),
        synthetic_workspaces(),
        Message::IcedEvent(Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(18.0, 110.0),
        })),
        Message::GaugeBatch(synthetic_gauge_batch()),
    ]
}

fn run_dispatch_rounds(state: &mut BarState, mix: &[Message], rounds: usize) -> usize {
    let mut task_units = 0usize;
    for _ in 0..rounds {
        for message in mix {
            task_units =
                task_units.saturating_add(runtime_dispatch::update(state, message.clone()).units());
        }
    }
    task_units
}

fn bench_main_loop_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("main_loop_dispatch");
    // Fixed criterion settings keep runs deterministic and explicit for CI.
    group.sample_size(60);
    group.warm_up_time(std::time::Duration::from_secs(1));
    group.measurement_time(std::time::Duration::from_secs(8));

    let mix = representative_mix();
    let scenarios = [
        DispatchScenario {
            name: "balanced_mix_64_rounds",
            rounds: 64,
        },
        DispatchScenario {
            name: "balanced_mix_256_rounds",
            rounds: 256,
        },
    ];

    for scenario in scenarios {
        group.bench_function(scenario.name, |b| {
            b.iter_batched(
                BarState::default,
                |mut state| {
                    let task_units = run_dispatch_rounds(&mut state, &mix, scenario.rounds);
                    black_box((task_units, state.gauges.len(), state.workspaces.len()));
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_main_loop_dispatch);
criterion_main!(benches);
