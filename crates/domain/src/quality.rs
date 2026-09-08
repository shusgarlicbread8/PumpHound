use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataQuality {
    pub grade: QualityGrade,
    pub score: f64,
    pub chain_lag_ms: Option<i64>,
    pub market_lag_ms: Option<i64>,
    pub ordering: OrderingQuality,
    pub coverage: Coverage,
    pub missing_fields: Vec<String>,
    pub stale_components: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityGrade {
    A,
    B,
    C,
    #[serde(rename = "UNAVAILABLE")]
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderingQuality {
    Exact,
    Provisional,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    CompleteForScope,
    Partial,
    Unknown,
}

impl DataQuality {
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            grade: QualityGrade::Unavailable,
            score: 0.0,
            chain_lag_ms: None,
            market_lag_ms: None,
            ordering: OrderingQuality::Unknown,
            coverage: Coverage::Unknown,
            missing_fields: vec![],
            stale_components: vec![],
            reasons: vec![reason.into()],
        }
    }

    pub fn live(chain_lag_ms: i64, market_lag_ms: i64, exact_order: bool) -> Self {
        let delayed = chain_lag_ms > 2000 || market_lag_ms > 1000;
        Self {
            grade: if delayed {
                QualityGrade::C
            } else if exact_order {
                QualityGrade::A
            } else {
                QualityGrade::B
            },
            score: if delayed {
                0.4
            } else if exact_order {
                0.95
            } else {
                0.7
            },
            chain_lag_ms: Some(chain_lag_ms),
            market_lag_ms: Some(market_lag_ms),
            ordering: if exact_order {
                OrderingQuality::Exact
            } else {
                OrderingQuality::Provisional
            },
            coverage: Coverage::CompleteForScope,
            missing_fields: vec![],
            stale_components: if delayed {
                vec!["chain_stream".into()]
            } else {
                vec![]
            },
            reasons: vec![],
        }
    }
}
