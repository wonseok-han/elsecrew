use elsecrew_core::proctree::Clock;
use std::{io, time::Duration};

// Only bridges OS termination observation to Child::try_wait readiness. This
// result never substitutes for the process-tree report or durable evidence.
pub(super) fn after_confirmation(
    confirmed: bool,
    mut try_reap: impl FnMut() -> io::Result<bool>,
    clock: &impl Clock,
) -> bool {
    let deadline = clock.elapsed_ms().saturating_add(1_000);
    loop {
        match try_reap() {
            Ok(true) => return confirmed,
            Err(_) => return false,
            Ok(false) => {}
        }
        let now = clock.elapsed_ms();
        if !confirmed || now >= deadline {
            return false;
        }
        clock.sleep(Duration::from_millis((deadline - now).min(25)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct TestClock(Cell<u64>);
    impl Clock for TestClock {
        fn elapsed_ms(&self) -> u64 {
            self.0.get()
        }
        fn sleep(&self, duration: Duration) {
            self.0.set(self.0.get() + duration.as_millis() as u64);
        }
    }

    #[test]
    fn confirmed_reap_handles_immediate_and_delayed_exit() {
        let clock = TestClock::default();
        assert!(after_confirmation(true, || Ok(true), &clock));
        assert_eq!(clock.elapsed_ms(), 0);
        assert!(after_confirmation(
            true,
            || Ok(clock.elapsed_ms() >= 75),
            &clock
        ));
        assert_eq!(clock.elapsed_ms(), 75);
    }

    #[test]
    fn confirmed_reap_timeout_remains_unconfirmed() {
        let clock = TestClock::default();
        assert!(!after_confirmation(true, || Ok(false), &clock));
        assert_eq!(clock.elapsed_ms(), 1_000);
    }

    #[test]
    fn uncertain_tree_and_wait_error_never_sleep() {
        let clock = TestClock::default();
        let calls = Cell::new(0);
        assert!(!after_confirmation(
            false,
            || {
                calls.set(calls.get() + 1);
                Ok(false)
            },
            &clock
        ));
        assert_eq!(calls.get(), 1);
        // Reap an already-exited direct child once, without presenting that as
        // confirmation of its whole process tree.
        assert!(!after_confirmation(false, || Ok(true), &clock));
        assert!(!after_confirmation(
            true,
            || Err(io::Error::other("wait failed")),
            &clock
        ));
        assert_eq!(clock.elapsed_ms(), 0);
    }
}
