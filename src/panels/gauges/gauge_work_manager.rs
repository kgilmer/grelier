// Gauge work-manager runtime and subscription adapter.
use crate::bar::Message;
use crate::icon::svg_asset;
use crate::panels::gauges::gauge::{Gauge, GaugeDisplay, GaugeModel, GaugeReadyNotify};
use crate::panels::gauges::gauge_registry;
use crate::settings;
use iced::Subscription;
use iced::futures::channel::mpsc;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc as sync_mpsc};
use std::thread;
use std::time::{Duration, Instant};

type GaugeBatchMessageStream = Box<dyn iced::futures::Stream<Item = Message> + Send + Unpin>;

/// Gauge subscription.
pub fn subscription(gauges: &[String]) -> Subscription<Message> {
    if gauges.is_empty() {
        return Subscription::none();
    }
    let gauge_ids: Arc<[String]> = gauges.iter().cloned().collect();
    Subscription::run_with(gauge_ids, gauge_batch_stream_by_ids)
}

fn gauge_batch_stream_by_ids(ids: &Arc<[String]>) -> GaugeBatchMessageStream {
    let (mut sender, receiver) = mpsc::channel(16);
    let ids = ids.clone();

    thread::spawn(move || {
        let now = Instant::now();
        let (ready_tx, ready_rx) = sync_mpsc::channel::<&'static str>();
        let ready_tx = Arc::new(Mutex::new(ready_tx));
        let ready_notify: GaugeReadyNotify = Arc::new(move |id| {
            if let Ok(ready_tx) = ready_tx.lock() {
                let _ = ready_tx.send(id);
            }
        });

        let mut gauges: Vec<Box<dyn Gauge>> = ids
            .iter()
            .filter_map(|id| gauge_registry::create_gauge(id, now))
            .collect();
        if gauges.is_empty() {
            return;
        }
        for gauge in &mut gauges {
            gauge.bind_ready_notify(ready_notify.clone());
        }

        let max_run_ms = settings::settings().get_parsed_or("grelier.gauge.work.max_run_ms", 40u64);
        let max_run_strikes =
            settings::settings().get_parsed_or("grelier.gauge.work.max_run_strikes", 3u8);
        let mut manager = GaugeWorkManager::new(
            SystemClock,
            Duration::from_millis(max_run_ms),
            max_run_strikes,
            gauges,
        );

        loop {
            let sleep_for = manager.next_wakeup_delay();
            pump_ready_notifications(&ready_rx, &mut manager, sleep_for);

            if let Some(batch) = manager.step_once() {
                let _ = sender.try_send(Message::GaugeBatch(batch));
            }
        }
    });

    Box::new(receiver)
}

fn pump_ready_notifications<C: Clock>(
    ready_rx: &sync_mpsc::Receiver<&'static str>,
    manager: &mut GaugeWorkManager<C>,
    sleep_for: Duration,
) {
    if sleep_for.is_zero() {
        drain_ready_notifications(ready_rx, manager);
        return;
    }

    match ready_rx.recv_timeout(sleep_for) {
        Ok(id) => {
            let _ = manager.mark_ready(id);
            drain_ready_notifications(ready_rx, manager);
        }
        Err(sync_mpsc::RecvTimeoutError::Timeout) => {}
        Err(sync_mpsc::RecvTimeoutError::Disconnected) => {}
    }
}

fn drain_ready_notifications<C: Clock>(
    ready_rx: &sync_mpsc::Receiver<&'static str>,
    manager: &mut GaugeWorkManager<C>,
) {
    while let Ok(id) = ready_rx.try_recv() {
        let _ = manager.mark_ready(id);
    }
}

/// Clock abstraction to make scheduling deterministic in unit tests.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Minimal fake clock for deterministic scheduler unit tests.
#[cfg(test)]
#[derive(Clone)]
pub struct FakeClock {
    base: Instant,
    offset: Arc<Mutex<Duration>>,
}

#[cfg(test)]
impl FakeClock {
    pub fn new(base: Instant) -> Self {
        Self {
            base,
            offset: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    pub fn advance(&self, by: Duration) {
        if let Ok(mut offset) = self.offset.lock() {
            *offset = offset.saturating_add(by);
        }
    }
}

#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> Instant {
        let offset = self.offset.lock().map(|d| *d).unwrap_or(Duration::ZERO);
        self.base + offset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeStatus {
    Active,
    Dead,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct GaugeRuntimeSnapshot {
    pub id: &'static str,
    pub status: GaugeStatus,
    pub next_deadline: Instant,
    pub strike_count: u8,
    pub run_count: u64,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ManagerSnapshot {
    pub now: Instant,
    pub deadline_heap_len: usize,
    pub ready_queue_len: usize,
    pub runtimes: Vec<GaugeRuntimeSnapshot>,
}

struct GaugeRuntime {
    gauge: Box<dyn Gauge>,
    status: GaugeStatus,
    next_deadline: Instant,
    generation: u64,
    strike_count: u8,
    run_count: u64,
}

/// Deterministic scheduler used by runtime and unit tests.
///
/// The manager runs gauges sequentially, enforces a per-run timeout policy,
/// and returns update batches for atomic UI application.
pub struct GaugeWorkManager<C: Clock> {
    clock: C,
    max_run: Duration,
    max_run_strikes: u8,
    runtimes: Vec<GaugeRuntime>,
    id_to_index: HashMap<&'static str, usize>,
    deadline_heap: BinaryHeap<Reverse<(Instant, usize, u64)>>,
    ready_queue: VecDeque<usize>,
    ready_set: BTreeSet<usize>,
}

impl<C: Clock> GaugeWorkManager<C> {
    /// Build a scheduler with the provided gauges.
    ///
    /// `max_run` and `max_run_strikes` control when slow gauges are transitioned to `Dead`.
    pub fn new(
        clock: C,
        max_run: Duration,
        max_run_strikes: u8,
        gauges: Vec<Box<dyn Gauge>>,
    ) -> Self {
        let mut runtimes = Vec::new();
        let mut id_to_index = HashMap::new();
        let mut deadline_heap = BinaryHeap::new();

        for (idx, gauge) in gauges.into_iter().enumerate() {
            let id = gauge.id();
            let next_deadline = gauge.next_deadline();
            let runtime = GaugeRuntime {
                gauge,
                status: GaugeStatus::Active,
                next_deadline,
                generation: 0,
                strike_count: 0,
                run_count: 0,
            };
            id_to_index.insert(id, idx);
            deadline_heap.push(Reverse((next_deadline, idx, 0)));
            runtimes.push(runtime);
        }

        Self {
            clock,
            max_run,
            max_run_strikes: max_run_strikes.max(1),
            runtimes,
            id_to_index,
            deadline_heap,
            ready_queue: VecDeque::new(),
            ready_set: BTreeSet::new(),
        }
    }

    pub fn mark_ready(&mut self, gauge_id: &str) -> bool {
        let Some(&idx) = self.id_to_index.get(gauge_id) else {
            return false;
        };
        if self.runtimes[idx].status == GaugeStatus::Dead {
            return false;
        }
        self.enqueue_ready_index(idx)
    }

    /// Delay until the scheduler should wake up again.
    ///
    /// Returns zero when at least one gauge is already ready to run.
    pub fn next_wakeup_delay(&self) -> Duration {
        if !self.ready_queue.is_empty() {
            return Duration::ZERO;
        }

        let now = self.clock.now();
        self.runtimes
            .iter()
            .filter(|runtime| runtime.status == GaugeStatus::Active)
            .map(|runtime| runtime.next_deadline.saturating_duration_since(now))
            .min()
            .unwrap_or_else(|| Duration::from_millis(250))
    }

    /// Run one scheduling cycle and return the emitted gauge update batch.
    ///
    /// Returns `None` when no gauge emitted a model in this cycle.
    pub fn step_once(&mut self) -> Option<Vec<GaugeModel>> {
        let now = self.clock.now();
        let mut runnable = BTreeSet::new();

        while let Some(Reverse((deadline, idx, generation))) = self.deadline_heap.peek().copied() {
            if deadline > now {
                break;
            }
            let _ = self.deadline_heap.pop();
            let runtime = &self.runtimes[idx];
            if runtime.status == GaugeStatus::Dead {
                continue;
            }
            if runtime.generation != generation || runtime.next_deadline != deadline {
                continue;
            }
            let _ = runnable.insert(idx);
        }

        while let Some(idx) = self.ready_queue.pop_front() {
            self.ready_set.remove(&idx);
            if self.runtimes[idx].status == GaugeStatus::Active {
                let _ = runnable.insert(idx);
            }
        }

        if runnable.is_empty() {
            return None;
        }

        let mut updates = Vec::new();
        for idx in runnable {
            let runtime = &mut self.runtimes[idx];
            if runtime.status == GaugeStatus::Dead {
                continue;
            }

            let started = self.clock.now();
            let maybe_model = runtime.gauge.run_once(now);
            let elapsed = self.clock.now().saturating_duration_since(started);
            runtime.run_count = runtime.run_count.saturating_add(1);

            if elapsed > self.max_run {
                runtime.strike_count = runtime.strike_count.saturating_add(1);
                if runtime.strike_count >= self.max_run_strikes {
                    runtime.status = GaugeStatus::Dead;
                    updates.push(dead_gauge_model(runtime.gauge.id()));
                    continue;
                }
            } else {
                runtime.strike_count = 0;
            }

            if let Some(model) = maybe_model {
                updates.push(model);
            }

            runtime.next_deadline = runtime.gauge.next_deadline();
            runtime.generation = runtime.generation.wrapping_add(1);
            self.deadline_heap
                .push(Reverse((runtime.next_deadline, idx, runtime.generation)));
        }

        if updates.is_empty() {
            None
        } else {
            Some(updates)
        }
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> ManagerSnapshot {
        ManagerSnapshot {
            now: self.clock.now(),
            deadline_heap_len: self.deadline_heap.len(),
            ready_queue_len: self.ready_queue.len(),
            runtimes: self
                .runtimes
                .iter()
                .map(|runtime| GaugeRuntimeSnapshot {
                    id: runtime.gauge.id(),
                    status: runtime.status,
                    next_deadline: runtime.next_deadline,
                    strike_count: runtime.strike_count,
                    run_count: runtime.run_count,
                })
                .collect(),
        }
    }

    fn enqueue_ready_index(&mut self, idx: usize) -> bool {
        if self.ready_set.insert(idx) {
            self.ready_queue.push_back(idx);
            true
        } else {
            false
        }
    }
}

fn dead_gauge_model(id: &'static str) -> GaugeModel {
    GaugeModel {
        id,
        icon: Some(svg_asset("turtle.svg")),
        display: GaugeDisplay::Empty,
        on_click: None,
        menu: None,
        action_dialog: None,
        info: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::gauges::gauge::GaugeDisplay;

    struct TestGauge {
        id: &'static str,
        clock: FakeClock,
        next_deadline: Instant,
        interval: Duration,
        run_duration: Duration,
        emit_model: bool,
    }

    impl TestGauge {
        fn new(
            id: &'static str,
            clock: FakeClock,
            next_deadline: Instant,
            interval: Duration,
            run_duration: Duration,
            emit_model: bool,
        ) -> Self {
            Self {
                id,
                clock,
                next_deadline,
                interval,
                run_duration,
                emit_model,
            }
        }
    }

    impl Gauge for TestGauge {
        fn id(&self) -> &'static str {
            self.id
        }

        fn next_deadline(&self) -> Instant {
            self.next_deadline
        }

        fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
            self.clock.advance(self.run_duration);
            self.next_deadline = now + self.interval;
            if self.emit_model {
                Some(GaugeModel {
                    id: self.id,
                    icon: None,
                    display: GaugeDisplay::Empty,
                    on_click: None,
                    menu: None,
                    action_dialog: None,
                    info: None,
                })
            } else {
                None
            }
        }
    }

    fn runtime<'a>(snapshot: &'a ManagerSnapshot, id: &str) -> &'a GaugeRuntimeSnapshot {
        snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.id == id)
            .unwrap()
    }

    #[test]
    fn due_only_execution_runs_only_due_gauges() {
        let start = Instant::now();
        let clock = FakeClock::new(start);
        let manager_clock = clock.clone();
        let mut manager = GaugeWorkManager::new(
            manager_clock,
            Duration::from_millis(40),
            3,
            vec![
                Box::new(TestGauge::new(
                    "g1",
                    clock.clone(),
                    start + Duration::from_millis(10),
                    Duration::from_millis(10),
                    Duration::ZERO,
                    true,
                )),
                Box::new(TestGauge::new(
                    "g2",
                    clock.clone(),
                    start + Duration::from_millis(20),
                    Duration::from_millis(10),
                    Duration::ZERO,
                    true,
                )),
            ],
        );

        assert!(manager.step_once().is_none());
        clock.advance(Duration::from_millis(10));
        let batch = manager.step_once().expect("first due gauge should emit");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].id, "g1");

        let snapshot = manager.snapshot();
        assert!(snapshot.deadline_heap_len >= 2);
        assert_eq!(snapshot.ready_queue_len, 0);
        assert!(snapshot.now >= start + Duration::from_millis(10));
        assert_eq!(runtime(&snapshot, "g1").run_count, 1);
        assert_eq!(runtime(&snapshot, "g2").run_count, 0);
        assert!(runtime(&snapshot, "g1").next_deadline > start);
    }

    #[test]
    fn ready_queue_can_run_before_deadline() {
        let start = Instant::now();
        let clock = FakeClock::new(start);
        let manager_clock = clock.clone();
        let mut manager = GaugeWorkManager::new(
            manager_clock,
            Duration::from_millis(40),
            3,
            vec![Box::new(TestGauge::new(
                "ready",
                clock.clone(),
                start + Duration::from_secs(60),
                Duration::from_secs(60),
                Duration::ZERO,
                true,
            ))],
        );

        assert!(manager.mark_ready("ready"));
        assert!(!manager.mark_ready("ready"));
        assert!(!manager.mark_ready("ready"));
        let batch = manager.step_once().expect("ready queue should run gauge");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].id, "ready");

        let snapshot = manager.snapshot();
        assert_eq!(runtime(&snapshot, "ready").run_count, 1);
    }

    #[test]
    fn ready_queue_deduplicates_gauges() {
        let start = Instant::now();
        let clock = FakeClock::new(start);
        let manager_clock = clock.clone();
        let mut manager = GaugeWorkManager::new(
            manager_clock,
            Duration::from_millis(40),
            3,
            vec![Box::new(TestGauge::new(
                "dup",
                clock.clone(),
                start + Duration::from_secs(60),
                Duration::from_secs(60),
                Duration::ZERO,
                true,
            ))],
        );

        assert!(manager.mark_ready("dup"));
        assert!(!manager.mark_ready("dup"));
        let _ = manager.step_once();

        let snapshot = manager.snapshot();
        assert_eq!(runtime(&snapshot, "dup").run_count, 1);
    }

    #[test]
    fn timeout_strikes_transition_gauge_to_dead() {
        let start = Instant::now();
        let clock = FakeClock::new(start);
        let manager_clock = clock.clone();
        let mut manager = GaugeWorkManager::new(
            manager_clock,
            Duration::from_millis(40),
            2,
            vec![Box::new(TestGauge::new(
                "slow",
                clock.clone(),
                start,
                Duration::from_millis(1),
                Duration::from_millis(50),
                true,
            ))],
        );

        assert!(manager.step_once().is_some());
        let first = manager.snapshot();
        assert_eq!(runtime(&first, "slow").status, GaugeStatus::Active);
        assert_eq!(runtime(&first, "slow").strike_count, 1);

        clock.advance(Duration::from_millis(1));
        let dead_batch = manager
            .step_once()
            .expect("dead transition should emit turtle model");
        assert_eq!(dead_batch.len(), 1);
        assert_eq!(dead_batch[0].id, "slow");
        assert_eq!(dead_batch[0].icon, Some(svg_asset("turtle.svg")));
        assert!(matches!(dead_batch[0].display, GaugeDisplay::Empty));
        let second = manager.snapshot();
        assert_eq!(runtime(&second, "slow").status, GaugeStatus::Dead);
        assert_eq!(runtime(&second, "slow").strike_count, 2);
        assert!(!manager.mark_ready("slow"));
    }

    #[test]
    fn system_clock_produces_non_decreasing_instant() {
        let clock = SystemClock;
        let first = clock.now();
        let second = clock.now();
        assert!(second >= first);
    }
}
