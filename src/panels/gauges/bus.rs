// Session-bus connection held across gauge ticks, with a bounded reconnect policy.
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

/// How long to leave the bus closed after failing to open it before trying again.
const OPEN_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

/// How long to wait before reconnecting after a call over the held connection failed.
///
/// A refused call is ambiguous — it can mean the connection died or only that the
/// service rejected the request — so this has to be short enough that a genuinely dead
/// connection recovers promptly, while still collapsing the reconnect cost from once
/// per tick to once per interval.
const RECONNECT_AFTER_FAILURE_COOLDOWN: Duration = Duration::from_secs(5);

/// One cached session bus connection, reopened when it goes away.
///
/// `Connection::system()` was paid once or twice per tick by every gauge doing D-Bus
/// work on its interval; holding one connection removes that from the tick entirely.
pub struct BusConnection {
    connection: Option<Connection>,
    /// Not before this instant will a failed open be retried.
    open_retry_deadline: Instant,
    /// Not before this instant will a reconnect following a failed call be attempted.
    reconnect_deadline: Instant,
}

impl Default for BusConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl BusConnection {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            connection: None,
            open_retry_deadline: now,
            reconnect_deadline: now,
        }
    }

    /// The held session bus, opening it when no cooldown is in force.
    pub fn connection_at(&mut self, now: Instant) -> Option<&Connection> {
        if self.connection.is_none() && now >= self.open_retry_deadline {
            if now < self.reconnect_deadline {
                return None;
            }
            self.connection = Connection::system().ok();
            if self.connection.is_none() {
                self.open_retry_deadline = now + OPEN_RETRY_COOLDOWN;
            }
        }
        self.connection.as_ref()
    }

    /// The held session bus, opening it now if any earlier cooldown has elapsed.
    pub fn connection(&mut self) -> Option<&Connection> {
        self.connection_at(Instant::now())
    }

    /// Whether the session bus itself can be reached, ignoring any cooldown left over
    /// from a failed call.
    ///
    /// For "should this UI exist at all" questions. A service refusing a request is not
    /// evidence the bus is gone, and gating UI on the method-call path makes it flicker
    /// once per cooldown. Only a failed *open* is rate-limited here.
    pub fn bus_available(&mut self) -> bool {
        if self.connection.is_some() {
            return true;
        }
        let now = Instant::now();
        if now < self.open_retry_deadline {
            return false;
        }
        self.connection = Connection::system().ok();
        if self.connection.is_none() {
            self.open_retry_deadline = now + OPEN_RETRY_COOLDOWN;
        }
        self.connection.is_some()
    }

    /// Run `call` against the held connection.
    ///
    /// A failure retires the connection and postpones the reconnect, because a failure
    /// can equally mean "the service refused this" as "the connection is dead" and a
    /// machine without the service must not pay a reconnect every tick. The cooldown is
    /// short so a genuinely dead connection recovers quickly, and it only ever hides a
    /// menu whose contents come from the same failing service.
    pub fn with_connection<T>(&mut self, call: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
        let Some(connection) = self.connection() else {
            // Already on cooldown from an earlier failure: report unavailable without
            // touching the deadlines, so the cooldown can actually elapse.
            return None;
        };
        let result = call(connection);
        if result.is_none() {
            self.connection = None;
            self.reconnect_deadline = Instant::now() + RECONNECT_AFTER_FAILURE_COOLDOWN;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_open_is_rate_limited() {
        let mut bus = BusConnection::new();
        let first = Instant::now();
        if bus.connection_at(first).is_some() {
            // A real session bus is present here; the cooldown path cannot be exercised.
            return;
        }
        assert!(
            bus.open_retry_deadline > first,
            "a failed open must postpone the next attempt"
        );
        assert!(
            bus.connection_at(first + Duration::from_secs(1)).is_none(),
            "the open cooldown window must not reopen the bus"
        );
    }

    #[test]
    fn failed_call_retires_the_connection_and_shortly_postpones_reconnect() {
        let mut bus = BusConnection::new();
        if bus.connection().is_none() {
            // No session bus: nothing held to retire.
            assert!(bus.open_retry_deadline > Instant::now());
            return;
        }
        let result: Option<u8> = bus.with_connection(|_| None);
        assert!(result.is_none());
        assert!(
            bus.connection.is_none(),
            "a failed call must retire the connection"
        );
        assert!(
            bus.reconnect_deadline > Instant::now(),
            "a failed call must not be retried on every tick"
        );
    }

    #[test]
    fn a_cooldown_is_not_extended_by_calls_that_never_ran() {
        // Wifi polls every 3s against a 5s cooldown. If a call made while the cooldown
        // was in force pushed it further out, the deadline could never elapse and a
        // transient bus failure would never recover.
        let mut bus = BusConnection::new();
        if bus.connection().is_none() {
            // No session bus here: the open cooldown, not the reconnect one, is in force.
            let deadline = bus.open_retry_deadline;
            for _ in 0..5 {
                let _: Option<u8> = bus.with_connection(|_| Some(1));
            }
            assert_eq!(
                bus.open_retry_deadline, deadline,
                "calls that never reached a connection must not move the open deadline"
            );
            return;
        }
        let _: Option<u8> = bus.with_connection(|_| None);
        let deadline = bus.reconnect_deadline;
        for _ in 0..5 {
            let _: Option<u8> = bus.with_connection(|_| Some(1));
        }
        assert_eq!(
            bus.reconnect_deadline, deadline,
            "calls that never reached a connection must not extend the cooldown"
        );
    }

    #[test]
    fn bus_available_ignores_the_reconnect_cooldown() {
        let mut bus = BusConnection::new();
        if !bus.bus_available() {
            // No session bus in this container: nothing to hold.
            return;
        }
        let _: Option<u8> = bus.with_connection(|_| None);
        assert!(
            bus.reconnect_deadline > Instant::now(),
            "precondition: the reconnect cooldown is in force"
        );
        assert!(
            bus.bus_available(),
            "a refused call must not make the bus itself look unreachable"
        );
    }

    #[test]
    fn reconnect_after_failure_is_shorter_than_the_open_cooldown() {
        // A dead connection has to recover well inside a wifi poll cadence, while still
        // collapsing repeated failures to one reconnect per interval.
        assert!(RECONNECT_AFTER_FAILURE_COOLDOWN < OPEN_RETRY_COOLDOWN);
        assert!(RECONNECT_AFTER_FAILURE_COOLDOWN >= Duration::from_secs(1));
    }
}
