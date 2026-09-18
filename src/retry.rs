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
            && let Some(delay) = error
                .api
                .as_ref()
                .and_then(|a| retry_after(&a.metadata.headers))
        {
            return delay;
        }
        if self.backoff_initial.is_zero() || self.backoff_max.is_zero() {
            return Duration::ZERO;
        }
        let base = self.backoff_initial.as_secs_f64() * 2f64.powf(f64::from(attempt));
        let base = base.min(self.backoff_max.as_secs_f64());
        let seconds = ((base * (1.0 - fastrand::f64() * self.backoff_jitter) * 1000.0).round()
            / 1000.0)
            .min(base);
        Duration::try_from_secs_f64(seconds).unwrap_or(self.backoff_max)
    }
}
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
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
                if let Ok(d) = Duration::try_from_secs_f64(n * multiplier) {
                    return Some(d);
                }
            }
            Ok(n) if n < 0.0 && name == "retry-after" => return None,
            Err(_) if name == "retry-after" => {
                if let Ok(date) = httpdate::parse_http_date(raw) {
                    return Some(date.duration_since(SystemTime::now()).unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    None
}
