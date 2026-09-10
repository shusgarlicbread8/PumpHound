use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    Pump,
    Hyperliquid,
    Ostium,
    Polymarket,
}

impl Venue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pump => "pump",
            Self::Hyperliquid => "hyperliquid",
            Self::Ostium => "ostium",
            Self::Polymarket => "polymarket",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    Perpetual,
    Spot,
    Outcome,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentId {
    pub venue: Venue,
    pub native_id: String,
}

impl InstrumentId {
    pub fn new(venue: Venue, native_id: impl Into<String>) -> Self {
        Self {
            venue,
            native_id: native_id.into(),
        }
    }

    pub fn render(&self) -> String {
        format!("{}:{}", self.venue.as_str(), self.native_id)
    }
}
