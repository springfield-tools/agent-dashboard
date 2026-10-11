/// GraphQL primary budget brakes (PLAN § Automatic brakes).
pub const PROJECTED_HOUR_CAP: i64 = 2500;
pub const LOW_REMAINING_SLOW_INTERVAL: u64 = 300;
pub const HIGH_COST_WARN: i32 = 50;
pub const LOW_REMAINING_WARN: i32 = 500;
pub const LOW_REMAINING_STOP: i32 = 100;

pub struct BrakeOutcome {
    pub sleep_secs: u64,
    pub banner: Option<String>,
    pub log_high_cost: bool,
}

/// `configured_interval` is Bryan's setting; returned sleep may be longer without changing config.
pub fn apply_brakes(
    configured_interval: u64,
    last_poll_cost: i32,
    rate_remaining: Option<i32>,
    rate_reset_at: Option<&str>,
    brake_slow_interval: u64,
) -> BrakeOutcome {
    let mut sleep_secs = configured_interval.max(brake_slow_interval);
    let mut banner = None;
    let log_high_cost = last_poll_cost > HIGH_COST_WARN;

    if last_poll_cost > 0 && configured_interval > 0 {
        let projected = last_poll_cost as i64 * (3600 / configured_interval as i64);
        if projected > PROJECTED_HOUR_CAP {
            let needed = ((last_poll_cost as i64 * 3600) + PROJECTED_HOUR_CAP - 1) / PROJECTED_HOUR_CAP;
            let needed = needed.max(configured_interval as i64) as u64;
            sleep_secs = sleep_secs.max(needed);
            banner = Some(format!(
                "Slowed to {}s to stay inside the GitHub rate limit (projected {} pts/hr).",
                needed,
                projected
            ));
        }
    }

    if let Some(remaining) = rate_remaining {
        if remaining < LOW_REMAINING_STOP {
            sleep_secs = sleep_secs.max(LOW_REMAINING_SLOW_INTERVAL);
            if let Some(reset) = rate_reset_at {
                banner = Some(format!(
                    "GraphQL budget very low ({remaining} remaining). Polling pauses until {reset}."
                ));
            }
        } else if remaining < LOW_REMAINING_WARN {
            sleep_secs = sleep_secs.max(LOW_REMAINING_SLOW_INTERVAL);
            if banner.is_none() {
                banner = Some(format!(
                    "GraphQL budget low ({remaining} remaining). Polling every 5 minutes until it recovers."
                ));
            }
        }
    }

    BrakeOutcome {
        sleep_secs,
        banner,
        log_high_cost,
    }
}

pub fn brake_slow_interval_for_cost(cost: i32, configured_interval: u64) -> u64 {
    if cost <= 0 || configured_interval == 0 {
        return 0;
    }
    let projected = cost as i64 * (3600 / configured_interval as i64);
    if projected <= PROJECTED_HOUR_CAP {
        return 0;
    }
    let needed = ((cost as i64 * 3600) + PROJECTED_HOUR_CAP - 1) / PROJECTED_HOUR_CAP;
    needed.max(configured_interval as i64) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slows_when_projected_hour_exceeds_cap() {
        let slow = brake_slow_interval_for_cost(15, 30);
        assert!(slow >= 30);
        let outcome = apply_brakes(30, 15, Some(4000), None, slow);
        assert!(outcome.sleep_secs >= 30);
        assert!(outcome.banner.is_some());
    }

    #[test]
    fn low_remaining_extends_to_five_minutes() {
        let outcome = apply_brakes(30, 2, Some(400), None, 0);
        assert_eq!(outcome.sleep_secs, LOW_REMAINING_SLOW_INTERVAL);
    }
}
