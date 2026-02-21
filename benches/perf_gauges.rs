use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use grelier::bar::DEFAULT_PANELS;
use grelier::panels::gauges::gauge::{GaugeRuntimeMode, scoped_gauge_runtime_mode};
use grelier::panels::gauges::gauge_registry::{self, GaugeSpec};
use grelier::settings;
use grelier::settings_storage::SettingsStorage;
use iced::futures::{StreamExt, executor::block_on};
use std::sync::Once;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

fn init_settings_once() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let mut path = std::env::temp_dir();
        path.push("grelier_perf_settings");
        path.push(format!(
            "Settings-bench-{}.xresources",
            env!("CARGO_PKG_VERSION")
        ));
        let storage = SettingsStorage::new(path);
        let settings_store = settings::Settings::new(storage);
        let _ = settings::init_settings(settings_store);

        let base_specs = settings::base_setting_specs(
            gauge_registry::default_gauges(),
            DEFAULT_PANELS,
            "left",
            "Nord",
        );
        let all_specs = gauge_registry::collect_settings(&base_specs);
        settings::settings().ensure_defaults(&all_specs);
    });
}

fn emits_quickly(spec: &'static GaugeSpec) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stream = (spec.stream)();
        let emitted = block_on(stream.next()).is_some() && block_on(stream.next()).is_some();
        let _ = tx.send(emitted);
    });
    rx.recv_timeout(PROBE_TIMEOUT).unwrap_or(false)
}

fn bench_polling_gauges(c: &mut Criterion) {
    init_settings_once();
    let _runtime_mode = scoped_gauge_runtime_mode(GaugeRuntimeMode::Benchmark);

    let mut polling_specs: Vec<&'static GaugeSpec> = gauge_registry::all()
        .filter(|spec| emits_quickly(spec))
        .collect();
    polling_specs.sort_by_key(|spec| spec.id);

    let mut group = c.benchmark_group("gauges_polling");
    group.throughput(Throughput::Elements(1));

    for spec in polling_specs {
        group.bench_with_input(BenchmarkId::new("first_tick", spec.id), &spec, |b, spec| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                for _ in 0..iters {
                    let mut stream = (spec.stream)();
                    let model = block_on(stream.next());
                    black_box(model);
                }
                start.elapsed()
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_polling_gauges);
criterion_main!(benches);
