use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchAge {
    pub seconds: Option<f64>,
    pub basis: &'static str,
}

/// Age is never invented. Chain create time wins; otherwise first-observed
/// (`known_at`) is used and labelled. Missing both → unknown, not `0`.
pub fn launch_age(
    creation_time: Option<DateTime<Utc>>,
    known_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> LaunchAge {
    if let Some(created) = creation_time.filter(|created| *created <= now) {
        return LaunchAge {
            seconds: Some(elapsed_seconds(created, now)),
            basis: "chain_create",
        };
    }
    if let Some(observed) = known_at.filter(|observed| *observed <= now) {
        return LaunchAge {
            seconds: Some(elapsed_seconds(observed, now)),
            basis: "first_observed",
        };
    }
    LaunchAge {
        seconds: None,
        basis: "unknown",
    }
}

fn elapsed_seconds(from: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    (now - from).num_milliseconds().max(0) as f64 / 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn missing_chain_time_uses_known_at_not_zero() {
        let now = DateTime::parse_from_rfc3339("2026-09-08T15:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let known = now - Duration::seconds(139);
        let age = launch_age(None, Some(known), now);
        assert_eq!(age.basis, "first_observed");
        assert!((age.seconds.unwrap() - 139.0).abs() < 0.01);
    }

    #[test]
    fn missing_both_is_unknown_not_zero() {
        let now = Utc::now();
        let age = launch_age(None, None, now);
        assert_eq!(age.basis, "unknown");
        assert_eq!(age.seconds, None);
    }

    #[test]
    fn chain_create_wins_over_known_at() {
        let now = DateTime::parse_from_rfc3339("2026-09-08T15:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let created = now - Duration::seconds(12);
        let known = now - Duration::seconds(4);
        let age = launch_age(Some(created), Some(known), now);
        assert_eq!(age.basis, "chain_create");
        assert!((age.seconds.unwrap() - 12.0).abs() < 0.01);
    }
}
