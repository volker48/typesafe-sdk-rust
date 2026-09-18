use crate::{Error, ErrorKind, HeaderMap};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

/// Retry counts are additional attempts. The budget stops *before* a retry;
/// it does not interrupt an in-flight attempt. Each call has independent state.
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
    pub backoff_jitter: f64,
    pub http_statuses: BTreeSet<u16>,
    pub respect_retry_after: bool,
    pub api_connection_error: bool,
    pub api_timeout_error: bool,
    pub timeout: Option<Duration>,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            backoff_initial: Duration::from_millis(500),
            backoff_max: Duration::from_secs(5),
            backoff_jitter: 0.25,
            http_statuses: [408, 429].into_iter().chain(500..600).collect(),
            respect_retry_after: true,
            api_connection_error: true,
            api_timeout_error: true,
            timeout: Some(Duration::from_secs(30)),
        }
    }
}
impl RetryPolicy {
    pub fn disabled() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !(0.0..=1.0).contains(&self.backoff_jitter) {
            return Err(Error::input("backoff_jitter must be between zero and one."));
        }
        if self.timeout.is_some_and(|d| d.is_zero()) {
            return Err(Error::input("Retry timeout must be positive."));
        }
        Ok(())
    }
    pub(crate) fn retryable(&self, error: &Error) -> bool {
        match error.kind {
            ErrorKind::Timeout => self.api_timeout_error,
            ErrorKind::Connection => self.api_connection_error,
            _ => error
                .api
                .as_ref()
                .is_some_and(|a| self.http_statuses.contains(&a.metadata.status)),
        }
    }
    pub(crate) fn delay(&self, attempt: u32, error: &Error) -> Duration {
        if self.respect_retry_after
            && let Some(delay) = error.api.as_ref().and_then(|a| a.retry_after)
        {
            return delay;
        }
        self.backoff(attempt, fastrand::f64())
    }
    fn backoff(&self, attempt: u32, random: f64) -> Duration {
        if self.backoff_initial.is_zero() || self.backoff_max.is_zero() {
            return Duration::ZERO;
        }
        let base = self.backoff_initial.as_secs_f64() * 2f64.powf(f64::from(attempt));
        let base = base.min(self.backoff_max.as_secs_f64());
        let seconds =
            ((base * (1.0 - random * self.backoff_jitter) * 1000.0).round() / 1000.0).min(base);
        Duration::try_from_secs_f64(seconds).unwrap_or(self.backoff_max)
    }
}
pub(crate) fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    for (name, multiplier) in [("retry-after-ms", 0.001), ("retry-after", 1.0)] {
        let Some(raw) = headers.get(name).and_then(|v| v.to_str().ok()) else {
            continue;
        };
        let raw = raw.trim();
        match if raw.is_empty() {
            Ok(0.0)
        } else {
            raw.parse::<f64>()
        } {
            Ok(n) if n.is_finite() && n >= 0.0 => {
                // A valid millisecond header wins even when its duration overflows.
                return Duration::try_from_secs_f64(n * multiplier).ok();
            }
            Ok(n) if n < 0.0 && name == "retry-after" => return None,
            Err(_) if name == "retry-after" => {
                if let Ok(date) = httpdate::parse_http_date(raw) {
                    return Some(date.duration_since(now).unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HeaderValue;

    #[test]
    fn http_dates_use_the_supplied_clock_and_clamp_past_dates() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        for (offset, expected) in [(10, 10), (-10, 0)] {
            let date = SystemTime::UNIX_EPOCH + Duration::from_secs((1_000_000 + offset) as u64);
            let headers = HeaderMap::from_iter([(
                crate::HeaderName::from_static("retry-after"),
                HeaderValue::from_str(&httpdate::fmt_http_date(date)).unwrap(),
            )]);
            assert_eq!(
                retry_after(&headers, now),
                Some(Duration::from_secs(expected))
            );
        }
    }

    #[test]
    fn exponential_backoff_caps_and_applies_subtractive_jitter() {
        let policy = RetryPolicy::default();
        for (attempt, seconds) in [
            (0, 0.5),
            (1, 1.0),
            (2, 2.0),
            (3, 4.0),
            (4, 5.0),
            (u32::MAX, 5.0),
        ] {
            assert_eq!(
                policy.backoff(attempt, 0.0),
                Duration::from_secs_f64(seconds)
            );
        }
        assert_eq!(policy.backoff(0, 1.0), Duration::from_millis(375));
        assert_eq!(policy.backoff(1, 0.5), Duration::from_millis(875));
        let small_cap = RetryPolicy {
            backoff_max: Duration::from_micros(600),
            ..policy.clone()
        };
        assert_eq!(small_cap.backoff(0, 0.0), Duration::from_micros(600));
        let zero_initial = RetryPolicy {
            backoff_initial: Duration::ZERO,
            ..policy.clone()
        };
        assert_eq!(zero_initial.backoff(10, 1.0), Duration::ZERO);
        let zero_cap = RetryPolicy {
            backoff_max: Duration::ZERO,
            ..policy
        };
        assert_eq!(zero_cap.backoff(10, 0.0), Duration::ZERO);
    }
}
