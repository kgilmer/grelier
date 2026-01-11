// Shared network sampling, formatting, and interval logic for net gauges.
// Consumes Settings: grelier.gauge.net.*.
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::settings;

/// Direction to compute a transfer rate for.
#[derive(Clone, Copy)]
pub enum RateDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

struct NetSample {
    iface: String,
    counters: NetCounters,
    timestamp: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetRates {
    pub upload_bytes_per_sec: f64,
    pub download_bytes_per_sec: f64,
}

/// Simple state machine to stretch sampling intervals when traffic is idle.
#[derive(Default)]
pub struct NetIntervalState {
    fast: bool,
    idle_streak: u8,
    config: NetIntervalConfig,
}

impl NetIntervalState {
    pub fn new(config: NetIntervalConfig) -> Self {
        Self {
            fast: false,
            idle_streak: 0,
            config,
        }
    }

    pub fn update(&mut self, rate: f64) {
        if rate > self.config.idle_threshold_bps {
            self.fast = true;
            self.idle_streak = 0;
        } else if self.fast {
            self.idle_streak = self.idle_streak.saturating_add(1);
            if self.idle_streak >= self.config.calm_ticks {
                self.fast = false;
                self.idle_streak = 0;
            }
        }
    }

    pub fn interval(&self) -> Duration {
        if self.fast {
            self.config.fast_interval
        } else {
            self.config.slow_interval
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NetIntervalConfig {
    pub idle_threshold_bps: f64,
    pub fast_interval: Duration,
    pub slow_interval: Duration,
    pub calm_ticks: u8,
}

impl Default for NetIntervalConfig {
    fn default() -> Self {
        Self {
            idle_threshold_bps: 10_240.0,
            fast_interval: Duration::from_secs(1),
            slow_interval: Duration::from_secs(3),
            calm_ticks: 4,
        }
    }
}

pub fn net_interval_config_from_settings() -> NetIntervalConfig {
    let idle_threshold_bps =
        settings::settings().get_parsed_or("grelier.gauge.net.idle_threshold_bps", 10_240.0);
    let fast_interval_secs =
        settings::settings().get_parsed_or("grelier.gauge.net.fast_interval_secs", 1u64);
    let slow_interval_secs =
        settings::settings().get_parsed_or("grelier.gauge.net.slow_interval_secs", 3u64);
    let calm_ticks = settings::settings().get_parsed_or("grelier.gauge.net.calm_ticks", 4u8);

    NetIntervalConfig {
        idle_threshold_bps,
        fast_interval: Duration::from_secs(fast_interval_secs),
        slow_interval: Duration::from_secs(slow_interval_secs),
        calm_ticks,
    }
}

const DEFAULT_IFACE_CACHE_TTL_SECS: u64 = 10;
const DEFAULT_IFACE_TTL_SECS: u64 = 5;
const DEFAULT_SAMPLER_MIN_INTERVAL_MS: u64 = 900;
const DEFAULT_SYS_CLASS_NET_PATH: &str = "/sys/class/net";
const DEFAULT_PROC_NET_ROUTE_PATH: &str = "/proc/net/route";
const DEFAULT_PROC_NET_DEV_PATH: &str = "/proc/net/dev";

#[derive(Clone, Copy, Debug)]
pub struct NetSamplerConfig {
    pub min_interval: Duration,
    pub iface_ttl: Duration,
}

impl NetSamplerConfig {
    pub fn default_settings() -> Self {
        Self {
            min_interval: Duration::from_millis(DEFAULT_SAMPLER_MIN_INTERVAL_MS),
            iface_ttl: Duration::from_secs(DEFAULT_IFACE_TTL_SECS),
        }
    }

    #[cfg(test)]
    fn with_timings(min_interval: Duration, iface_ttl: Duration) -> Self {
        Self {
            min_interval,
            iface_ttl,
        }
    }
}

pub fn sampler_config_from_settings() -> NetSamplerConfig {
    let min_interval_ms = settings::settings().get_parsed_or(
        "grelier.gauge.net.sampler_min_interval_ms",
        DEFAULT_SAMPLER_MIN_INTERVAL_MS,
    );
    let iface_ttl_secs = settings::settings()
        .get_parsed_or("grelier.gauge.net.iface_ttl_secs", DEFAULT_IFACE_TTL_SECS);

    NetSamplerConfig {
        min_interval: Duration::from_millis(min_interval_ms),
        iface_ttl: Duration::from_secs(iface_ttl_secs),
    }
}

/// Provides interface name, counters, and time for rate computation.
pub(crate) trait NetDataProvider: Send {
    fn now(&mut self) -> Instant;
    fn active_interface(&mut self) -> Option<String>;
    fn read_counters(&mut self, iface: &str) -> Option<NetCounters>;
}

pub(crate) struct SystemNetProvider {
    cached_iface: Option<String>,
    last_iface_check: Option<Instant>,
}

impl NetDataProvider for SystemNetProvider {
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn active_interface(&mut self) -> Option<String> {
        let now = Instant::now();
        let cache_ttl = iface_cache_ttl();
        if let (Some(iface), Some(last_check)) = (self.cached_iface.clone(), self.last_iface_check)
            && now.duration_since(last_check) < cache_ttl
            && interface_is_up(&iface)
        {
            return Some(iface);
        }

        let detected = active_interface_scan()?;
        self.cached_iface = Some(detected.clone());
        self.last_iface_check = Some(now);
        Some(detected)
    }

    fn read_counters(&mut self, iface: &str) -> Option<NetCounters> {
        read_counters(iface)
    }
}

/// Tracks a shared set of counters and reuses fresh samples to avoid duplicate `/proc` reads.
pub struct NetSampler<P: NetDataProvider = SystemNetProvider> {
    provider: P,
    last_sample: Option<NetSample>,
    last_rates: Option<NetRates>,
    last_at: Option<Instant>,
    min_interval: Duration,
    last_iface: Option<String>,
    last_iface_check: Option<Instant>,
    iface_ttl: Duration,
}

impl NetSampler<SystemNetProvider> {
    pub fn new() -> Self {
        Self::with_provider_and_config(
            SystemNetProvider {
                cached_iface: None,
                last_iface_check: None,
            },
            sampler_config_from_settings(),
        )
    }
}

impl<P: NetDataProvider> NetSampler<P> {
    pub fn with_provider_and_config(provider: P, config: NetSamplerConfig) -> Self {
        Self {
            provider,
            last_sample: None,
            last_rates: None,
            last_at: None,
            min_interval: config.min_interval,
            last_iface: None,
            last_iface_check: None,
            iface_ttl: config.iface_ttl,
        }
    }

    pub fn with_provider(provider: P) -> Self {
        Self::with_provider_and_config(provider, NetSamplerConfig::default_settings())
    }

    #[cfg(test)]
    fn with_timings(provider: P, min_interval: Duration, iface_ttl: Duration) -> Self {
        Self::with_provider_and_config(
            provider,
            NetSamplerConfig::with_timings(min_interval, iface_ttl),
        )
    }

    /// Returns upload/download bytes per second, refreshing counters only if the cached sample
    /// is older than `min_interval`. This keeps upload/download gauges from triggering separate
    /// `/proc/net` reads on the same second.
    pub fn rates(&mut self) -> Option<NetRates> {
        let now = self.provider.now();
        if let Some(last_at) = self.last_at
            && now.duration_since(last_at) < self.min_interval
        {
            return self.last_rates;
        }

        let iface = match (
            self.last_iface.clone(),
            self.last_iface_check,
            self.iface_ttl,
        ) {
            (Some(iface), Some(last_check), ttl) if now.duration_since(last_check) < ttl => iface,
            _ => {
                let iface = self.provider.active_interface()?;
                self.last_iface = Some(iface.clone());
                self.last_iface_check = Some(now);
                iface
            }
        };

        let counters = self.provider.read_counters(&iface)?;

        let rates = match &self.last_sample {
            Some(previous) if previous.iface == iface => {
                let elapsed = now.duration_since(previous.timestamp).as_secs_f64();
                if elapsed <= 0.0 {
                    NetRates {
                        upload_bytes_per_sec: 0.0,
                        download_bytes_per_sec: 0.0,
                    }
                } else {
                    let tx_delta = counters.tx_bytes.saturating_sub(previous.counters.tx_bytes);
                    let rx_delta = counters.rx_bytes.saturating_sub(previous.counters.rx_bytes);
                    NetRates {
                        upload_bytes_per_sec: tx_delta as f64 / elapsed,
                        download_bytes_per_sec: rx_delta as f64 / elapsed,
                    }
                }
            }
            _ => NetRates {
                upload_bytes_per_sec: 0.0,
                download_bytes_per_sec: 0.0,
            },
        };

        self.last_sample = Some(NetSample {
            iface,
            counters,
            timestamp: now,
        });
        self.last_rates = Some(rates);
        self.last_at = Some(now);

        Some(rates)
    }
}

static SHARED_NET_SAMPLER: OnceLock<Arc<Mutex<NetSampler>>> = OnceLock::new();

pub fn shared_net_sampler() -> Arc<Mutex<NetSampler>> {
    SHARED_NET_SAMPLER
        .get_or_init(|| Arc::new(Mutex::new(NetSampler::new())))
        .clone()
}

/// Format bytes/sec into KB/MB/GB per second, scaling to keep the number under three digits.
/// Format bytes/sec into a compact multi-line string: first line is two-digit value, second line is
/// a two-letter unit. Values are scaled to stay within two digits; non-zero values round up to a
/// minimum of `01` after scaling.
pub fn format_rate(bytes_per_sec: f64) -> String {
    const STEP: f64 = 1024.0;

    let mut value = bytes_per_sec.max(0.0) / STEP; // Start at KB.
    let mut unit = "KB";

    for next in ["MB", "GB", "TB"] {
        if value < 100.0 {
            break;
        }
        value /= STEP;
        unit = next;
    }

    let mut rounded = (value + 0.5).floor();
    if rounded == 0.0 && bytes_per_sec > 0.0 {
        rounded = 1.0;
    }
    if rounded > 99.0 {
        rounded = 99.0;
    }

    format!("{:02.0}\n{unit}", rounded)
}

/// Attempt to find the interface that carries the default route.
fn default_route_interface() -> Option<String> {
    let contents = fs::read_to_string(proc_net_route_path()).ok()?;
    for line in contents.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }

        let destination = fields[1];
        let mask = fields[7];
        let flags = u16::from_str_radix(fields[3], 16).ok().unwrap_or(0);

        if destination == "00000000" && mask == "00000000" && (flags & 0x1 != 0) {
            return Some(fields[0].to_string());
        }
    }
    None
}

/// Fallback: pick the first non-loopback interface that is up (and, if present, has carrier).
fn first_up_interface() -> Option<String> {
    let base = sys_class_net_path();
    for entry in fs::read_dir(base).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }

        let operstate = fs::read_to_string(entry.path().join("operstate"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if operstate != "up" {
            continue;
        }

        // If carrier is reported, require it to be present.
        if let Ok(carrier) = fs::read_to_string(entry.path().join("carrier"))
            && carrier.trim() == "0"
        {
            continue;
        }

        return Some(name);
    }
    None
}

pub fn active_interface() -> Option<String> {
    active_interface_scan()
}

fn active_interface_scan() -> Option<String> {
    default_route_interface().or_else(first_up_interface)
}

pub fn read_counters(iface: &str) -> Option<NetCounters> {
    let contents = fs::read_to_string(proc_net_dev_path()).ok()?;
    for line in contents.lines().skip(2) {
        let trimmed = line.trim();
        let Some(pos) = trimmed.find(':') else {
            continue;
        };

        let (name, rest) = trimmed.split_at(pos);
        if name.trim() != iface {
            continue;
        }

        let mut parts = rest[1..].split_whitespace();
        let rx_bytes: u64 = parts.next()?.parse().ok()?;

        // Skip to tx_bytes (9th field after the interface name).
        for _ in 0..7 {
            parts.next()?;
        }

        let tx_bytes: u64 = parts.next()?.parse().ok()?;

        return Some(NetCounters { rx_bytes, tx_bytes });
    }

    None
}

fn interface_is_up(iface: &str) -> bool {
    let base = sys_class_net_path().join(iface);
    let operstate = fs::read_to_string(base.join("operstate"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if operstate != "up" {
        return false;
    }

    if let Ok(carrier) = fs::read_to_string(base.join("carrier"))
        && carrier.trim() == "0"
    {
        return false;
    }

    true
}

fn iface_cache_ttl() -> Duration {
    let ttl_secs = settings::settings().get_parsed_or(
        "grelier.gauge.net.iface_cache_ttl_secs",
        DEFAULT_IFACE_CACHE_TTL_SECS,
    );
    Duration::from_secs(ttl_secs)
}

fn sys_class_net_path() -> PathBuf {
    PathBuf::from(settings::settings().get_or(
        "grelier.gauge.net.sys_class_net_path",
        DEFAULT_SYS_CLASS_NET_PATH,
    ))
}

fn proc_net_route_path() -> PathBuf {
    PathBuf::from(settings::settings().get_or(
        "grelier.gauge.net.proc_net_route_path",
        DEFAULT_PROC_NET_ROUTE_PATH,
    ))
}

fn proc_net_dev_path() -> PathBuf {
    PathBuf::from(settings::settings().get_or(
        "grelier.gauge.net.proc_net_dev_path",
        DEFAULT_PROC_NET_DEV_PATH,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;

    #[test]
    fn formats_small_and_large_rates() {
        assert_eq!(format_rate(0.0), "00\nKB");
        assert_eq!(format_rate(10_240.0), "10\nKB");
        assert_eq!(format_rate(150_000.0), "01\nMB");
        assert_eq!(format_rate(5_000_000.0), "05\nMB");
        assert_eq!(format_rate(5_000_000_000.0), "05\nGB");
    }

    struct FakeProvider {
        clock: Arc<Mutex<Instant>>,
        iface: String,
        samples: Vec<NetCounters>,
        reads: Arc<AtomicUsize>,
        iface_calls: Arc<AtomicUsize>,
    }

    impl FakeProvider {
        fn new(
            iface: &str,
            samples: Vec<NetCounters>,
            clock: Arc<Mutex<Instant>>,
            reads: Arc<AtomicUsize>,
            iface_calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                clock,
                iface: iface.to_string(),
                samples,
                reads,
                iface_calls,
            }
        }
    }

    impl NetDataProvider for FakeProvider {
        fn now(&mut self) -> Instant {
            *self.clock.lock().unwrap()
        }

        fn active_interface(&mut self) -> Option<String> {
            self.iface_calls.fetch_add(1, Ordering::SeqCst);
            Some(self.iface.clone())
        }

        fn read_counters(&mut self, _iface: &str) -> Option<NetCounters> {
            let idx = self.reads.fetch_add(1, Ordering::SeqCst);
            self.samples.get(idx).copied()
        }
    }

    #[test]
    fn sampler_reuses_recent_sample_and_computes_both_rates() {
        let start = Instant::now();
        let clock = Arc::new(Mutex::new(start));
        let reads = Arc::new(AtomicUsize::new(0));
        let iface_calls = Arc::new(AtomicUsize::new(0));
        let samples = vec![
            NetCounters {
                rx_bytes: 0,
                tx_bytes: 0,
            },
            NetCounters {
                rx_bytes: 800,
                tx_bytes: 400,
            },
        ];

        let provider = FakeProvider::new(
            "eth0",
            samples,
            clock.clone(),
            reads.clone(),
            iface_calls.clone(),
        );
        let mut sampler =
            NetSampler::with_timings(provider, Duration::from_millis(400), Duration::from_secs(2));

        let first = sampler.rates().expect("initial sample");
        assert_eq!(reads.load(Ordering::SeqCst), 1, "reads first counters");
        assert_eq!(first.upload_bytes_per_sec, 0.0);
        assert_eq!(first.download_bytes_per_sec, 0.0);

        {
            let mut now = clock.lock().unwrap();
            *now += Duration::from_millis(200);
        }
        let cached = sampler.rates().expect("cached sample");
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "should reuse recent counters"
        );
        assert_eq!(cached, first);

        {
            let mut now = clock.lock().unwrap();
            *now += Duration::from_millis(600); // total 800ms since first sample
        }
        let updated = sampler.rates().expect("second sample");
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "should refresh after min interval"
        );
        assert!(
            (updated.download_bytes_per_sec - 1000.0).abs() < 0.1,
            "download rate should be close to 1000 B/s, got {}",
            updated.download_bytes_per_sec
        );
        assert!(
            (updated.upload_bytes_per_sec - 500.0).abs() < 0.1,
            "upload rate should be close to 500 B/s, got {}",
            updated.upload_bytes_per_sec
        );
    }

    #[test]
    fn sampler_reuses_interface_detection_within_ttl() {
        let start = Instant::now();
        let clock = Arc::new(Mutex::new(start));
        let reads = Arc::new(AtomicUsize::new(0));
        let iface_calls = Arc::new(AtomicUsize::new(0));
        let samples = vec![
            NetCounters {
                rx_bytes: 0,
                tx_bytes: 0,
            },
            NetCounters {
                rx_bytes: 1_000,
                tx_bytes: 500,
            },
            NetCounters {
                rx_bytes: 2_000,
                tx_bytes: 1_000,
            },
        ];

        let provider = FakeProvider::new(
            "eth0",
            samples,
            clock.clone(),
            reads.clone(),
            iface_calls.clone(),
        );
        let mut sampler =
            NetSampler::with_timings(provider, Duration::from_millis(0), Duration::from_secs(1));

        let _ = sampler.rates().expect("first sample");
        assert_eq!(iface_calls.load(Ordering::SeqCst), 1);

        {
            let mut now = clock.lock().unwrap();
            *now += Duration::from_millis(500);
        }
        let _ = sampler.rates().expect("second sample");
        assert_eq!(
            iface_calls.load(Ordering::SeqCst),
            1,
            "should reuse cached interface within ttl"
        );

        {
            let mut now = clock.lock().unwrap();
            *now += Duration::from_millis(600);
        }
        let _ = sampler.rates().expect("third sample after ttl");
        assert_eq!(
            iface_calls.load(Ordering::SeqCst),
            2,
            "should refresh interface after ttl"
        );
    }

    #[test]
    fn net_interval_slows_after_idle_and_resumes_on_activity() {
        let mut state = NetIntervalState::new(NetIntervalConfig::default());

        // Activity above threshold -> fast interval.
        state.update(20_000.0);
        assert_eq!(state.interval(), Duration::from_secs(1));

        // A few idle ticks keep it fast.
        for _ in 0..3 {
            state.update(100.0);
            assert_eq!(state.interval(), Duration::from_secs(1));
        }

        // Next idle tick drops to slow interval.
        state.update(100.0);
        assert_eq!(state.interval(), Duration::from_secs(3));

        // Activity flips back to fast.
        state.update(50_000.0);
        assert_eq!(state.interval(), Duration::from_secs(1));
    }
}
