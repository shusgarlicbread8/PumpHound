use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{InstrumentId, Venue};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservationId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationTime {
    pub source_ts: Option<DateTime<Utc>>,
    pub received_ts: DateTime<Utc>,
}

impl ObservationTime {
    pub fn receive_delay_ms(self) -> Option<i64> {
        self.source_ts
            .map(|source| self.received_ts.signed_duration_since(source).num_milliseconds())
    }
}

/// Observed numeric value. Missing stays missing; a live `0` is stored as `Some(0.0)`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OptionalNumber(pub Option<f64>);

impl OptionalNumber {
    pub fn unknown() -> Self {
        Self(None)
    }

    pub fn observed(value: f64) -> Self {
        Self(Some(value))
    }

    pub fn from_str(raw: Option<&str>) -> Self {
        match raw {
            Some(text) if !text.is_empty() => text.parse().ok().map(Self::observed).unwrap_or_else(Self::unknown),
            _ => Self::unknown(),
        }
    }

    pub fn is_unknown(self) -> bool {
        self.0.is_none()
    }

    pub fn as_f64(self) -> Option<f64> {
        self.0
    }
}

impl Default for OptionalNumber {
    fn default() -> Self {
        Self::unknown()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketObservationMeta {
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub sequence: Option<u64>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub mid: OptionalNumber,
}

impl MarketObservationMeta {
    pub fn new(venue: Venue, native_id: impl Into<String>, sequence: Option<u64>) -> Self {
        Self {
            venue,
            instrument_id: InstrumentId::new(venue, native_id),
            sequence,
            source_timestamp: None,
            received_at: Utc::now(),
            mid: OptionalNumber::unknown(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResearchStatus {
    InsufficientSample,
    Confounded,
    SampleOnly,
    Rejected,
    Unproven,
}
