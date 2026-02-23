# Contributing

## Gauge Development

This project runs all gauges through a shared scheduler. To add or modify a gauge,
you should understand the types below first.

### Core Types

- `src/panels/gauges/gauge.rs`
  - `Gauge`: runtime trait every gauge implements.
  - `GaugeWake`: why a gauge run was triggered (`Timer` vs `ExternalEvent`).
  - `RunOutcome`: result of `Gauge::run` (`NoChange` or `ModelChanged`).
  - `GaugeEventSource`: external event producer owned by the work manager.
  - `GaugeRegistrar`: registration hook used by gauges to attach event sources.
  - `GaugeModel`: full UI model for a gauge render/update.
  - `GaugeDisplay`, `GaugeValue`, `GaugeValueAttention`: value rendering semantics.
  - `GaugeClick`, `GaugeInput`, `GaugeClickAction`: pointer input payloads/callbacks.
  - `GaugeReadyNotify`: callback for requesting immediate scheduler wakeup.

- `src/panels/gauges/gauge_registry.rs`
  - `GaugeSpec`: static metadata registered via `inventory::submit!`.
  - `GaugeFactory`: function signature used to construct a runtime gauge.
  - `create_gauge`: runtime constructor lookup by id.

- `src/panels/gauges/gauge_work_manager.rs`
  - `GaugeWorkManager`: sequential scheduler with timeout/dead-gauge policy.
  - `subscription`: entry point that wires selected gauges into batched updates.
  - `GaugeStatus`: gauge lifecycle state (`Active`/`Dead`).

- `src/bar.rs`
  - `Message::GaugeBatch(Vec<GaugeModel>)`: batched UI update message applied atomically.

### Add a New Gauge

1. Create a module under `src/panels/gauges/` (or edit an existing one).
2. Implement a state struct and `impl Gauge` for it.
3. If the gauge has external events (inotify, dbus, Pulse, udev, etc.), define a
   `GaugeEventSource` and register it from `Gauge::register`.
4. Add a `create_gauge(now: Instant) -> Box<dyn Gauge>` factory in that module.
5. Register a `GaugeSpec` with `inventory::submit!`, including:
   - `id`
   - `description`
   - `default_enabled`
   - `settings`
   - `create: create_gauge`
   - optional `validate`
6. Export the module from `src/panels/gauges/mod.rs` if needed.

### Behavior Guidelines

- Keep `run`/`run_once` bounded and predictable; slow gauges can be marked dead by policy.
- Use `GaugeReadyNotify` for immediate reruns after local command/input events.
- Register external event sources via `Gauge::register`; do not spawn unmanaged threads.
- Return `RunOutcome::NoChange`/`None` when no visual update is needed.
- Keep gauge ids stable; ids are used in settings and routing.

### Validation

Run this before opening a change:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```
