use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::AuthError;

const WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: u8 = 5;
const MAX_TRACKED_PHONES: usize = 10_000;

#[derive(Default)]
pub(super) struct LoginThrottle {
    attempts: BTreeMap<String, Arc<Mutex<Attempts>>>,
}

#[derive(Default)]
pub(super) struct Attempts {
    started_at: Option<Instant>,
    failures: u8,
}

impl LoginThrottle {
    pub(super) fn for_phone(
        &mut self,
        phone: &str,
        now: Instant,
    ) -> Result<Arc<Mutex<Attempts>>, AuthError> {
        if !self.attempts.contains_key(phone) && self.attempts.len() >= MAX_TRACKED_PHONES {
            self.attempts.retain(|_, attempts| {
                // Keep in-flight and queued requests attached to the same lock.
                Arc::strong_count(attempts) > 1
                    || attempts
                        .try_lock()
                        .map_or(true, |attempts| attempts.active(now))
            });
            // Do not evict a live limit: rotating phone numbers must not clear it.
            if self.attempts.len() >= MAX_TRACKED_PHONES {
                return Err(AuthError::TooManyAttempts);
            }
        }
        Ok(self.attempts.entry(phone.to_string()).or_default().clone())
    }
}

impl Attempts {
    fn active(&self, now: Instant) -> bool {
        self.started_at
            .is_some_and(|started| now.duration_since(started) < WINDOW)
    }

    pub(super) fn check(&mut self, now: Instant) -> Result<(), AuthError> {
        if !self.active(now) {
            self.reset();
        }
        if self.failures >= MAX_FAILURES {
            return Err(AuthError::TooManyAttempts);
        }
        Ok(())
    }

    pub(super) fn record_failure(&mut self, now: Instant) {
        if !self.active(now) {
            self.reset();
        }
        self.started_at.get_or_insert(now);
        self.failures += 1;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_failures_without_extending_the_window_and_recovers_at_expiry() {
        let mut attempts = Attempts::default();
        let now = Instant::now();
        for _ in 0..MAX_FAILURES {
            assert_eq!(attempts.check(now), Ok(()));
            attempts.record_failure(now);
        }
        assert_eq!(
            attempts.check(now + WINDOW - Duration::from_millis(1)),
            Err(AuthError::TooManyAttempts)
        );
        assert_eq!(attempts.check(now + WINDOW), Ok(()));
    }

    #[test]
    fn capacity_is_bounded_without_evicting_active_limits() {
        let mut throttle = LoginThrottle::default();
        let now = Instant::now();
        for i in 0..MAX_TRACKED_PHONES {
            throttle
                .for_phone(&format!("+99890{i:07}"), now)
                .unwrap()
                .try_lock()
                .unwrap()
                .record_failure(now);
        }
        assert!(matches!(
            throttle.for_phone("new-phone", now),
            Err(AuthError::TooManyAttempts)
        ));
        assert_eq!(throttle.attempts.len(), MAX_TRACKED_PHONES);
        assert!(throttle.for_phone("new-phone", now + WINDOW).is_ok());
        assert_eq!(throttle.attempts.len(), 1);
    }

    #[test]
    fn cleanup_preserves_in_flight_phone_locks() {
        let mut throttle = LoginThrottle::default();
        let now = Instant::now();
        let active = throttle.for_phone("active", now).unwrap();
        for i in 1..MAX_TRACKED_PHONES {
            throttle.for_phone(&format!("+99890{i:07}"), now).unwrap();
        }
        throttle.for_phone("new-phone", now + WINDOW).unwrap();
        let same_phone = throttle.for_phone("active", now + WINDOW).unwrap();
        assert!(Arc::ptr_eq(&active, &same_phone));
        assert_eq!(throttle.attempts.len(), 2);
    }
}
