use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Venue;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthState {
    Healthy,
    Stale,
    Degraded,
    DisabledMissingKey,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataHealth {
    pub state: HealthState,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub stale_after_ms: u64,
}

impl DataHealth {
    pub fn unknown() -> Self {
        Self {
            state: HealthState::Unknown,
            last_success_at: None,
            last_error: None,
            stale_after_ms: 60_000,
        }
    }

    pub fn evaluate(&self, now: DateTime<Utc>) -> HealthState {
        if matches!(
            self.state,
            HealthState::DisabledMissingKey | HealthState::Unsupported
        ) {
            return self.state;
        }
        if let Some(last) = self.last_success_at {
            let age = now.signed_duration_since(last).num_milliseconds();
            if age > self.stale_after_ms as i64 {
                return HealthState::Stale;
            }
        }
        self.state
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub provider: String,
    pub venue: Venue,
    pub health: DataHealth,
    pub error_count_1h: u32,
    pub queue_depth: Option<u32>,
}
