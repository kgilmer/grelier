use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use grelier::gauges::gauge::{Gauge, GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention};
use grelier::gauges::gauge_work_manager::{Clock, GaugeWorkManager};

#[derive(Clone)]
struct BenchClock {
    base: Instant,
    offset_ms: Arc<AtomicU64>,
}

impl BenchClock {
    fn new(base: Instant) -> Self {
        Self {
            base,
            offset_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    fn advance_ms(&self, by: u64) {
        self.offset_ms.fetch_add(by, Ordering::Relaxed);
    }
}

impl Clock for BenchClock {
    fn now(&self) -> Instant {
        let offset = self.offset_ms.load(Ordering::Relaxed);
        self.base + Duration::from_millis(offset)
    }
}

struct SyntheticGauge {
    id: &'static str,
    next_deadline: Instant,
    interval: Duration,
    spin_ops: u32,
    value: u64,
}

impl SyntheticGauge {
    fn new(id: &'static str, now: Instant, interval: Duration, spin_ops: u32) -> Self {
        Self {
            id,
            next_deadline: now,
            interval,
            spin_ops,
            value: 1,
        }
    }
}

impl Gauge for SyntheticGauge {
    fn id(&self) -> &'static str {
        self.id
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let mut v = self.value;
        for _ in 0..self.spin_ops {
            v = v.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        self.value = v;
        self.next_deadline = now + self.interval;

        Some(GaugeModel {
            id: self.id,
            icon: None,
            display: GaugeDisplay::Value {
                value: GaugeValue::Text((self.value % 100).to_string()),
                attention: GaugeValueAttention::Nominal,
            },
            on_click: None,
            menu: None,
            action_dialog: None,
            info: None,
        })
    }
}

#[derive(Clone, Copy)]
struct GaugeScenario {
    name: &'static str,
    gauge_count: usize,
    spin_ops: u32,
    interval_ms: u64,
    ticks: usize,
}

fn regression_canary_enabled() -> bool {
    // CI regression-canary mode: intentionally make this benchmark slower so the
    // perf gate can prove it catches regressions. This is opt-in and only enabled
    // by the dedicated non-blocking self-test job.
    std::env::var("GRELIER_PERF_REGRESSION_CANARY")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn build_manager(
    s: GaugeScenario,
) -> (GaugeWorkManager<BenchClock>, BenchClock, Vec<&'static str>) {
    let clock = BenchClock::new(Instant::now());
    let now = clock.now();
    let mut ids = Vec::with_capacity(s.gauge_count);
    let mut gauges: Vec<Box<dyn Gauge>> = Vec::with_capacity(s.gauge_count);

    for i in 0..s.gauge_count {
        let id = Box::leak(format!("synthetic_{i}").into_boxed_str());
        ids.push(id as &'static str);
        let stagger = Duration::from_millis((i as u64 % 4) * 25);
        let interval = Duration::from_millis(s.interval_ms);
        let mut gauge = SyntheticGauge::new(id, now + stagger, interval, s.spin_ops);
        gauge.next_deadline = now + stagger;
        gauges.push(Box::new(gauge));
    }

    let manager = GaugeWorkManager::new(clock.clone(), Duration::from_millis(250), 3, gauges);
    (manager, clock, ids)
}

fn run_ticks(
    manager: &mut GaugeWorkManager<BenchClock>,
    clock: &BenchClock,
    ids: &[&str],
    ticks: usize,
) -> usize {
    let mut emitted_models = 0usize;

    for tick in 0..ticks {
        // Explicit mix: periodic deadline advancement plus regular ready notifications.
        clock.advance_ms(50);
        if tick % 5 == 0 {
            let idx = tick % ids.len();
            let _ = manager.mark_ready(ids[idx]);
        }
        if let Some(batch) = manager.step_once() {
            emitted_models = emitted_models.saturating_add(batch.len());
        }
    }

    emitted_models
}

fn bench_gauge_work_manager(c: &mut Criterion) {
    let mut group = c.benchmark_group("gauge_work_manager");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(8));

    let canary = regression_canary_enabled();
    // Keep normal benchmark behavior unchanged by default; only inflate work in
    // canary mode to force a predictable regression signal in CI.
    let canary_spin_multiplier = if canary { 64 } else { 1 };

    let scenarios = [
        GaugeScenario {
            name: "periodic_light_8_gauges",
            gauge_count: 8,
            spin_ops: 200 * canary_spin_multiplier,
            interval_ms: 250,
            ticks: 250,
        },
        GaugeScenario {
            name: "periodic_mixed_24_gauges",
            gauge_count: 24,
            spin_ops: 500 * canary_spin_multiplier,
            interval_ms: 500,
            ticks: 500,
        },
    ];

    for scenario in scenarios {
        group.bench_function(scenario.name, |b| {
            b.iter_batched(
                || build_manager(scenario),
                |(mut manager, clock, ids)| {
                    let emitted = run_ticks(&mut manager, &clock, &ids, scenario.ticks);
                    black_box(emitted);
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_gauge_work_manager);
criterion_main!(benches);
