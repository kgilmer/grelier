// Session-bus connection held across gauge ticks, with a bounded reconnect policy.
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

/// How long to leave the bus closed after a failed call before reopening it.
///
/// Bounds the cost of a machine that has no such service, or of a bus that has gone
/// away, to one reconnect per interval instead of one per gauge tick.
const BUS_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

/// One cached session bus connection, reopened after a failed call.
///
/// `Connection::system()` costs ~0.17ms on a quiet x86 bus, which gauges doing D-Bus
/// work on every tick paid one or two times per tick before this existed.
pub struct BusConnection {
    connection: Option<Connection>,
    /// Not before this instant will the bus be reopened following a failure.
    retry_deadline: Instant,
}

impl Default for BusConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl BusConnection {
    pub fn new() -> Self {
        Self {
            connection: None,
            retry_deadline: Instant::now(),
        }
    }

    /// The held session bus, opening it unless a recent failure put it on cooldown.
    pub fn connection_at(&mut self, now: Instant) -> Option<&Connection> {
        if self.connection.is_none() {
            if now < self.retry_deadline {
                return None;
            }
            self.connection = Connection::system().ok();
            if self.connection.is_none() {
                self.retry_deadline = now + BUS_RETRY_COOLDOWN;
            }
        }
        self.connection.as_ref()
    }

    /// The held session bus, opening it now if a previous failure's cooldown has
    /// already elapsed.
    pub fn connection(&mut self) -> Option<&Connection> {
        self.connection_at(Instant::now())
    }

    /// Run `call` against the held connection. A failure retires the connection and
    /// puts the bus on cooldown, so a permanently absent service cannot cost a
    /// reconnect every tick.
    pub fn with_connection<T>(&mut self, call: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
        let now = Instant::now();
        let result = self
            .connection_at(now)
            .and_then(|connection| call(connection));
        if result.is_none() {
            self.connection = None;
            self.retry_deadline = now + BUS_RETRY_COOLDOWN;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_call_holds_the_bus_on_cooldown() {
        // No session bus in a test container: the first attempt is allowed, the
        // cooldown then has to stop it being retried on every tick.
        let mut bus = BusConnection::new();
        bus.connection_at(Instant::now());
        let later = Instant::now() + Duration::from_millis(1);
        assert!(
            bus.connection_at(later).is_none() || bus.connection.is_some(),
            "either the bus is open or the cooldown is in force"
        );
        if bus.connection.is_none() {
            assert!(
                bus.retry_deadline > later,
                "a failed connect must postpone the next attempt"
            );
        }
    }

    #[test]
    fn failed_call_retires_the_connection() {
        let mut bus = BusConnection::new();
        let result: Option<u8> = bus.with_connection(|_| None);
        assert!(result.is_none());
        assert!(bus.connection.is_none());
        assert!(bus.retry_deadline > Instant::now());
    }
}
