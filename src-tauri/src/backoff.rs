use chrono::{DateTime, Utc};

/// Failure classes from PLAN § Backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    PrimaryRateLimit,
    SecondaryRateLimit,
    TransientHttp,
}

const TRANSIENT_STEPS: [u64; 4] = [30, 60, 120, 240];
const TRANSIENT_CAP: u64 = 300;

pub(crate) fn jitter_seconds(base: u64, nanos: u32) -> u64 {
    if base == 0 {
        return 0;
    }
    let jitter = (nanos as u64) % (base / 4 + 1);
    base + jitter
}

pub fn primary_limit_wait_until_reset(reset_at: &str, nanos: u32) -> Option<DateTime<Utc>> {
    let until = DateTime::parse_from_rfc3339(reset_at)
        .map(|t| t.with_timezone(&Utc))
        .ok()?;
    let extra = 1 + (nanos as u64 % 5);
    Some(until + chrono::Duration::seconds(extra as i64))
}

pub fn next_transient_backoff(current: u64) -> u64 {
    if current == 0 {
        TRANSIENT_STEPS[0]
    } else {
        for step in TRANSIENT_STEPS {
            if current < step {
                return step;
            }
        }
        TRANSIENT_CAP
    }
}

pub fn wait_seconds_for_failure(
    kind: FailureKind,
    transient_backoff: u64,
    retry_after_secs: Option<u64>,
    reset_at: Option<&str>,
    nanos: u32,
) -> u64 {
    match kind {
        FailureKind::PrimaryRateLimit => {
            if let Some(reset) = reset_at {
                if let Some(until) = primary_limit_wait_until_reset(reset, nanos) {
                    let secs = (until - Utc::now()).num_seconds();
                    return jitter_seconds(secs.max(1) as u64, nanos);
                }
            }
            jitter_seconds(300, nanos)
        }
        FailureKind::SecondaryRateLimit => {
            let base = retry_after_secs.unwrap_or_else(|| next_transient_backoff(transient_backoff));
            jitter_seconds(base, nanos)
        }
        FailureKind::TransientHttp => jitter_seconds(next_transient_backoff(transient_backoff), nanos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_backoff_steps_then_caps() {
        assert_eq!(next_transient_backoff(0), 30);
        assert_eq!(next_transient_backoff(30), 60);
        assert_eq!(next_transient_backoff(240), 300);
        assert_eq!(next_transient_backoff(300), 300);
    }

    #[test]
    fn primary_reset_adds_small_jitter() {
        let reset = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
        let until = primary_limit_wait_until_reset(&reset, 0).expect("parse");
        assert!(until > Utc::now() + chrono::Duration::minutes(9));
    }
}
