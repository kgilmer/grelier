// Session-bus connection held across gauge ticks, with a bounded reconnect policy.
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

/// How long to leave the bus closed after failing to open it before trying again.
///
/// Bounds the cost of a machine with no session bus to one open attempt per interval
/// instead of one per gauge tick.
const BUS_OPEN_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

/// One cached session bus connection, reopened when it goes away.
///
/// `Connection::system()` was paid once or twice per tick by every gauge doing D-Bus
/// work on its interval; holding one connection removes that from the tick entirely.
pub struct BusConnection {
    connection: Option<Connection>,
    /// Not before this instant will a failed open be retried.
    open_retry_deadline: Instant,
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
            open_retry_deadline: Instant::now(),
        }
    }

    /// The held session bus, opening it unless a recent failure put opening on cooldown.
    pub fn connection_at(&mut self, now: Instant) -> Option<&Connection> {
        if self.connection.is_none() && now >= self.open_retry_deadline {
            self.connection = Connection::system().ok();
            if self.connection.is_none() {
                self.open_retry_deadline = now + BUS_OPEN_RETRY_COOLDOWN;
            }
        }
        self.connection.as_ref()
    }

    /// The held session bus, opening it now if any earlier cooldown has elapsed.
    pub fn connection(&mut self) -> Option<&Connection> {
        self.connection_at(Instant::now())
    }

    /// Run `call` against the held connection.
    ///
    /// A failure retires the connection so the next call reconnects, but does **not**
    /// start the open cooldown: a service that refuses a request (NetworkManager not
    /// managing the interface, no such device, power-profiles-daemon absent) is not
    /// evidence the bus is gone, and blacking the bus out for a cooldown would turn a
    /// method error into a visible feature outage. Only a failed *open* is rate-limited.
    pub fn with_connection<T>(&mut self, call: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
        let connection = self.connection()?;
        let result = call(connection);
        if result.is_none() {
            self.connection = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_open_is_rate_limited() {
        // No session bus in a test container: the first attempt is allowed and the
        // cooldown then has to stop it being retried on every tick.
        let mut bus = BusConnection::new();
        let first = Instant::now();
        let opened = bus.connection_at(first);
        if opened.is_some() {
            // A real bus is present; nothing to assert about the cooldown path.
            return;
        }
        assert!(
            bus.open_retry_deadline > first,
            "a failed open must postpone the next attempt"
        );
        assert!(
            bus.connection_at(first + Duration::from_secs(1)).is_none(),
            "the cooldown window must not reopen the bus"
        );
    }

    #[test]
    fn failed_call_retires_the_connection_without_a_cooldown() {
        let mut bus = BusConnection::new();
        // Force the "connection held" state by opening it if a bus exists here.
        if bus.connection().is_none() {
            // No bus: a failed open does leave a cooldown, which is the other test.
            assert!(bus.open_retry_deadline > Instant::now());
            return;
        }
        let result: Option<u8> = bus.with_connection(|_| None);
        assert!(result.is_none());
        assert!(bus.connection.is_none());
        assert!(
            bus.open_retry_deadline <= Instant::now(),
            "a refused call must not rate-limit reopening the bus"
        );
    }
}
