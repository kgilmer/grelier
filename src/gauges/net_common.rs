use std::fs;
use std::path::Path;
use std::time::Instant;

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

/// Tracks the last network sample to compute deltas between ticks.
#[derive(Default)]
pub struct NetRateTracker {
    last: Option<NetSample>,
}

impl NetRateTracker {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Returns the current bytes/sec for the requested direction on the active interface.
    pub fn rate(&mut self, direction: RateDirection) -> Option<f64> {
        let iface = active_interface()?;
        let counters = read_counters(&iface)?;
        let now = Instant::now();

        let rate = match &self.last {
            Some(previous) if previous.iface == iface => {
                let elapsed = now.duration_since(previous.timestamp).as_secs_f64();
                if elapsed <= 0.0 {
                    0.0
                } else {
                    let delta = match direction {
                        RateDirection::Upload => {
                            counters.tx_bytes.saturating_sub(previous.counters.tx_bytes)
                        }
                        RateDirection::Download => {
                            counters.rx_bytes.saturating_sub(previous.counters.rx_bytes)
                        }
                    };
                    delta as f64 / elapsed
                }
            }
            // Interface changed or no previous sample; seed the tracker.
            _ => 0.0,
        };

        self.last = Some(NetSample {
            iface,
            counters,
            timestamp: now,
        });

        Some(rate)
    }
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
    let contents = fs::read_to_string("/proc/net/route").ok()?;
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
    let base = Path::new("/sys/class/net");
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
        if let Ok(carrier) = fs::read_to_string(entry.path().join("carrier")) {
            if carrier.trim() == "0" {
                continue;
            }
        }

        return Some(name);
    }
    None
}

pub fn active_interface() -> Option<String> {
    default_route_interface().or_else(first_up_interface)
}

pub fn read_counters(iface: &str) -> Option<NetCounters> {
    let contents = fs::read_to_string("/proc/net/dev").ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_small_and_large_rates() {
        assert_eq!(format_rate(0.0), "00\nKB");
        assert_eq!(format_rate(10_240.0), "10\nKB");
        assert_eq!(format_rate(150_000.0), "01\nMB");
        assert_eq!(format_rate(5_000_000.0), "05\nMB");
        assert_eq!(format_rate(5_000_000_000.0), "05\nGB");
    }
}
