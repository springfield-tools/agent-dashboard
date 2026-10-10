use chrono::{DateTime, Days, Duration, Local, TimeZone, Timelike, Utc};

/// Whether GitHub polling should be skipped for user pause.
pub fn is_polling_paused(poll_pause: &Option<String>) -> bool {
    match poll_pause {
        None => false,
        Some(value) if value == "running" => false,
        Some(value) if value == "manual" => true,
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map(|until| until.with_timezone(&Utc) > Utc::now())
            .unwrap_or(true),
    }
}

pub fn pause_manual_value() -> String {
    "manual".to_string()
}

pub fn pause_for_one_hour_value() -> String {
    (Utc::now() + Duration::hours(1)).to_rfc3339()
}

pub fn pause_until_tomorrow_local_value() -> String {
    let local = Local::now();
    let tomorrow = local.date_naive() + Days::new(1);
    let midnight = tomorrow.and_hms_opt(0, 0, 0).expect("midnight");
    local
        .timezone()
        .from_local_datetime(&midnight)
        .single()
        .unwrap_or_else(|| local.timezone().from_utc_datetime(&midnight))
        .to_rfc3339()
}

/// Clears a timed pause that has expired. Returns true when the config was changed.
pub fn clear_expired_timed_pause(poll_pause: &mut Option<String>) -> bool {
    match poll_pause.as_deref() {
        None | Some("manual") | Some("running") => false,
        Some(_) if is_polling_paused(poll_pause) => false,
        Some(_) => {
            *poll_pause = None;
            true
        }
    }
}

/// Seconds until a timed pause expires; `None` for manual pause or when not paused.
pub fn seconds_until_timed_resume(poll_pause: &Option<String>) -> Option<u64> {
    match poll_pause {
        Some(value) if value == "manual" || value == "running" => None,
        Some(value) => DateTime::parse_from_rfc3339(value).ok().map(|until| {
            let secs = (until.with_timezone(&Utc) - Utc::now()).num_seconds();
            if secs <= 0 {
                1
            } else {
                secs as u64
            }
        }),
        None => None,
    }
}

pub fn pause_tooltip(poll_pause: &Option<String>) -> String {
    match poll_pause {
        None | Some(_) if !is_polling_paused(poll_pause) => "Polling".to_string(),
        Some(value) if value == "manual" => "Polling paused".to_string(),
        Some(value) => {
            if let Ok(until) = DateTime::parse_from_rfc3339(value) {
                let local = until.with_timezone(&Local);
                if local.date_naive() > Local::now().date_naive() {
                    return format!(
                        "Polling paused until tomorrow ({:02}:{:02})",
                        local.hour(),
                        local.minute()
                    );
                }
                return format!(
                    "Polling paused until {:02}:{:02}",
                    local.hour(),
                    local.minute()
                );
            }
            "Polling paused".to_string()
        }
        _ => "Polling paused".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_pause_is_active() {
        assert!(is_polling_paused(&Some("manual".to_string())));
    }

    #[test]
    fn future_timestamp_pauses() {
        let until = (Utc::now() + Duration::minutes(30)).to_rfc3339();
        assert!(is_polling_paused(&Some(until)));
    }

    #[test]
    fn past_timestamp_resumes() {
        let until = (Utc::now() - Duration::minutes(5)).to_rfc3339();
        assert!(!is_polling_paused(&Some(until)));
    }
}
