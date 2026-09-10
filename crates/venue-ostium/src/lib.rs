//! Ostium public/read-only research adapter. No trading.

use chrono::{DateTime, Utc};
use pumphound_venue_core::{InstrumentId, OptionalNumber, Venue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const APP_PAIRS_URL: &str = "https://app.ostium.io/api/pairs";
pub const BUILDER_PAIRS_URL: &str = "https://builder.prod.bedrock.ostium.io/v1/pairs";

#[derive(Debug, Error)]
pub enum OstiumError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("decode: {0}")]
    Decode(String),
}

#[derive(Clone)]
pub struct OstiumClient {
    http: reqwest::Client,
}

impl Default for OstiumClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .user_agent("pumphound-research/0.1")
                .build()
                .expect("ostium http"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OstiumMarket {
    pub instrument_id: InstrumentId,
    pub pair_id: String,
    pub symbol: String,
    pub from_asset: Option<String>,
    pub to_asset: Option<String>,
    pub group: Option<String>,
    pub price: OptionalNumber,
    pub price_source: Option<String>,
    pub funding_long: OptionalNumber,
    pub funding_short: OptionalNumber,
    pub last_funding_rate: OptionalNumber,
    pub rollover: OptionalNumber,
    pub rollover_long_acc: OptionalNumber,
    pub rollover_short_acc: OptionalNumber,
    pub broker_premium: OptionalNumber,
    pub is_negative_rollover_allowed: Option<bool>,
    pub oi_long: OptionalNumber,
    pub oi_short: OptionalNumber,
    pub max_oi_usd: OptionalNumber,
    pub volume_usd: OptionalNumber,
    pub max_leverage: OptionalNumber,
    pub min_leverage: OptionalNumber,
    pub utilization: OptionalNumber,
    pub price_impact_k: OptionalNumber,
    pub sim_slippage_status: String,
    pub raw: Value,
}

impl OstiumClient {
    pub async fn fetch_merged(&self) -> Result<Vec<OstiumMarket>, OstiumError> {
        let builder = self.get_json(BUILDER_PAIRS_URL).await?;
        let app = self.get_json(APP_PAIRS_URL).await?;
        merge_pairs(&builder, &app)
    }

    async fn get_json(&self, url: &str) -> Result<Value, OstiumError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| OstiumError::Transport(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| OstiumError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(OstiumError::Transport(format!(
                "HTTP {status}: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| OstiumError::Decode(e.to_string()))
    }
}

pub fn merge_pairs(builder: &Value, app: &Value) -> Result<Vec<OstiumMarket>, OstiumError> {
    let builder_rows = builder_pairs(builder)?;
    let app_rows = app_pairs(app)?;
    let mut by_id = std::collections::BTreeMap::<String, (Option<&Value>, Option<&Value>)>::new();
    for row in &builder_rows {
        let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        by_id.entry(id.to_string()).or_default().0 = Some(row);
    }
    for row in &app_rows {
        let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        by_id.entry(id.to_string()).or_default().1 = Some(row);
    }
    Ok(by_id
        .into_iter()
        .map(|(id, (builder_row, app_row))| normalize_pair(&id, builder_row, app_row))
        .collect())
}

fn builder_pairs(value: &Value) -> Result<Vec<&Value>, OstiumError> {
    value
        .get("pairs")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().collect())
        .ok_or_else(|| OstiumError::Decode("builder.pairs missing".into()))
}

fn app_pairs(value: &Value) -> Result<Vec<&Value>, OstiumError> {
    value
        .as_array()
        .map(|rows| rows.iter().collect())
        .ok_or_else(|| OstiumError::Decode("app pairs must be an array".into()))
}

fn normalize_pair(id: &str, builder: Option<&Value>, app: Option<&Value>) -> OstiumMarket {
    let symbol = builder
        .and_then(|v| v.get("symbol"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let from = builder.or(app).and_then(|v| v.get("from")).and_then(Value::as_str);
            let to = builder.or(app).and_then(|v| v.get("to")).and_then(Value::as_str);
            match (from, to) {
                (Some(a), Some(b)) => Some(format!("{a}-{b}")),
                _ => None,
            }
        })
        .unwrap_or_else(|| id.to_string());
    let mut raw = serde_json::json!({});
    if let Some(v) = builder {
        raw["builder"] = v.clone();
    }
    if let Some(v) = app {
        raw["app"] = v.clone();
    }
    OstiumMarket {
        instrument_id: InstrumentId::new(Venue::Ostium, id),
        pair_id: id.to_string(),
        symbol,
        from_asset: text(builder.or(app), "from"),
        to_asset: text(builder.or(app), "to"),
        group: builder
            .and_then(|v| v.get("group"))
            .and_then(|g| g.get("name"))
            .and_then(Value::as_str)
            .or_else(|| {
                app.and_then(|v| v.get("group"))
                    .and_then(|g| g.get("name"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string),
        price: first_num(&[
            (builder, "lastTradePrice"),
        ]),
        price_source: builder
            .and_then(|_| Some("builder.lastTradePrice".into())),
        funding_long: first_num(&[(app, "curFundingLong")]),
        funding_short: first_num(&[(app, "curFundingShort")]),
        last_funding_rate: first_num(&[(builder, "lastFundingRate"), (app, "lastFundingRate")]),
        rollover: first_num(&[(builder, "curRollover"), (app, "curRollover")]),
        rollover_long_acc: first_num(&[(app, "accRolloverLong")]),
        rollover_short_acc: first_num(&[(app, "accRolloverShort")]),
        broker_premium: first_num(&[(app, "brokerPremium")]),
        is_negative_rollover_allowed: app
            .and_then(|v| v.get("isNegativeRolloverAllowed"))
            .and_then(Value::as_bool),
        oi_long: first_num(&[(builder, "longOIInUnits")]),
        oi_short: first_num(&[(builder, "shortOIInUnits")]),
        max_oi_usd: first_num(&[(builder, "maxOIInUsd")]),
        volume_usd: first_num(&[(builder, "volumeInUsd")]),
        max_leverage: first_num(&[(builder, "maxLeverage")]),
        min_leverage: first_num(&[(builder, "minLeverage")]),
        utilization: utilization(builder),
        price_impact_k: first_num(&[(app, "priceImpactK")]),
        sim_slippage_status: "UNSUPPORTED".into(),
        raw,
    }
}

fn utilization(builder: Option<&Value>) -> OptionalNumber {
    let Some(builder) = builder else {
        return OptionalNumber::unknown();
    };
    let (Some(long), Some(short), Some(max_oi), Some(px)) = (
        num(builder.get("longOIInUnits")).as_f64(),
        num(builder.get("shortOIInUnits")).as_f64(),
        num(builder.get("maxOIInUsd")).as_f64(),
        num(builder.get("lastTradePrice")).as_f64(),
    ) else {
        return OptionalNumber::unknown();
    };
    if max_oi <= 0.0 || px <= 0.0 {
        return OptionalNumber::unknown();
    }
    OptionalNumber::observed(((long + short) * px) / max_oi)
}

fn text(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn first_num(rows: &[(Option<&Value>, &str)]) -> OptionalNumber {
    for (value, key) in rows {
        if let Some(v) = value.and_then(|row| row.get(*key)) {
            let parsed = num(Some(v));
            if !parsed.is_unknown() {
                return parsed;
            }
        }
    }
    OptionalNumber::unknown()
}

fn num(value: Option<&Value>) -> OptionalNumber {
    match value {
        Some(Value::Null) | None => OptionalNumber::unknown(),
        Some(Value::String(s)) => OptionalNumber::from_str(Some(s)),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(OptionalNumber::observed)
            .unwrap_or_else(OptionalNumber::unknown),
        Some(Value::Bool(_)) => OptionalNumber::unknown(),
        Some(_) => OptionalNumber::unknown(),
    }
}

pub fn parse_time_secs(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_i64)
        .or_else(|| value.and_then(Value::as_str).and_then(|s| s.parse().ok()))
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn funding_zero_is_observed_zero_not_unknown() {
        let builder = json!({"pairs":[{"id":"0","symbol":"BTC-USD","from":"BTC","to":"USD","lastTradePrice":77213.58,"longOIInUnits":1.06,"shortOIInUnits":0.41,"maxOIInUsd":5_000_000.0,"lastFundingRate":0,"curRollover":0}]});
        let app = json!([{
            "id":"0","from":"BTC","to":"USD",
            "curFundingLong":"0","curFundingShort":"0","curRollover":"0",
            "isNegativeRolloverAllowed":true,"accRolloverLong":"1","accRolloverShort":"2",
            "brokerPremium":"238000000","priceImpactK":"1"
        }]);
        let rows = merge_pairs(&builder, &app).unwrap();
        assert_eq!(rows[0].funding_long.as_f64(), Some(0.0));
        assert_eq!(rows[0].funding_short.as_f64(), Some(0.0));
        assert_eq!(rows[0].rollover.as_f64(), Some(0.0));
        assert_eq!(rows[0].is_negative_rollover_allowed, Some(true));
        assert_eq!(rows[0].sim_slippage_status, "UNSUPPORTED");
        assert_ne!(rows[0].funding_long.as_f64(), rows[0].rollover.as_f64().filter(|_| false));
    }

    #[test]
    fn missing_builder_price_stays_unknown() {
        let builder = json!({"pairs":[{"id":"1","symbol":"X-USD"}]});
        let app = json!([{"id":"1","curFundingLong":"0"}]);
        let rows = merge_pairs(&builder, &app).unwrap();
        assert!(rows[0].price.is_unknown());
        assert_eq!(rows[0].funding_long.as_f64(), Some(0.0));
    }
}
