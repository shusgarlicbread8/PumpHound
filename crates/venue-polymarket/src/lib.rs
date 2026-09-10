//! Polymarket public/read-only research adapter. No trading, no market making.

use chrono::{DateTime, Utc};
use pumphound_venue_core::{InstrumentId, OptionalNumber, Venue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const GAMMA_MARKETS_URL: &str = "https://gamma-api.polymarket.com/markets";
pub const CLOB_BOOK_URL: &str = "https://clob.polymarket.com/book";

#[derive(Debug, Error)]
pub enum PolymarketError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("decode: {0}")]
    Decode(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RelationProvenance {
    ProvenRelation,
    LikelyRelation,
    UnverifiedRelation,
}

#[derive(Clone)]
pub struct PolymarketClient {
    http: reqwest::Client,
}

impl Default for PolymarketClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .user_agent("pumphound-research/0.1")
                .build()
                .expect("polymarket http"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PredictionMarket {
    pub instrument_id: InstrumentId,
    pub market_id: String,
    pub question: String,
    pub condition_id: Option<String>,
    pub event_id: Option<String>,
    pub event_title: Option<String>,
    pub category: Option<String>,
    pub yes_price: OptionalNumber,
    pub no_price: OptionalNumber,
    pub best_bid: OptionalNumber,
    pub best_ask: OptionalNumber,
    pub spread: OptionalNumber,
    pub volume: OptionalNumber,
    pub liquidity: OptionalNumber,
    pub end_date: Option<DateTime<Utc>>,
    pub resolution_source: Option<String>,
    pub resolution_rules: Option<String>,
    pub resolved_by: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub yes_token_id: Option<String>,
    pub no_token_id: Option<String>,
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PredictionEvent {
    pub event_id: String,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClobBook {
    pub token_id: String,
    pub market: Option<String>,
    pub source_ts: Option<DateTime<Utc>>,
    pub best_bid: OptionalNumber,
    pub best_ask: OptionalNumber,
    pub mid: OptionalNumber,
    pub spread: OptionalNumber,
    pub bids: Vec<(OptionalNumber, OptionalNumber)>,
    pub asks: Vec<(OptionalNumber, OptionalNumber)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketRelation {
    pub left_id: String,
    pub right_id: String,
    pub relation_type: String,
    pub provenance: RelationProvenance,
    pub evidence: Value,
}

impl PolymarketClient {
    pub async fn active_markets(&self, limit: u32) -> Result<Vec<PredictionMarket>, PolymarketError> {
        let url = format!("{GAMMA_MARKETS_URL}?limit={limit}&active=true&closed=false");
        let value = self.get_json(&url).await?;
        parse_markets(&value)
    }

    pub async fn book(&self, token_id: &str) -> Result<ClobBook, PolymarketError> {
        let url = format!("{CLOB_BOOK_URL}?token_id={token_id}");
        let value = self.get_json(&url).await?;
        parse_book(token_id, &value)
    }

    async fn get_json(&self, url: &str) -> Result<Value, PolymarketError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| PolymarketError::Transport(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| PolymarketError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(PolymarketError::Transport(format!(
                "HTTP {status}: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| PolymarketError::Decode(e.to_string()))
    }
}

pub fn parse_markets(value: &Value) -> Result<Vec<PredictionMarket>, PolymarketError> {
    let rows = value
        .as_array()
        .ok_or_else(|| PolymarketError::Decode("gamma markets must be an array".into()))?;
    Ok(rows.iter().filter_map(parse_market).collect())
}

pub fn parse_market(row: &Value) -> Option<PredictionMarket> {
    let market_id = row.get("id")?.as_str()?.to_string();
        let prices = parse_json_array(row.get("outcomePrices"));
        let event = row
        .get("events")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    Some(PredictionMarket {
        instrument_id: InstrumentId::new(Venue::Polymarket, &market_id),
        market_id,
        question: row
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        condition_id: text(row, "conditionId"),
        event_id: event.and_then(|e| e.get("id")).and_then(Value::as_str).map(str::to_string),
        event_title: event
            .and_then(|e| e.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string),
        category: event
            .and_then(|e| e.get("category"))
            .and_then(Value::as_str)
            .map(str::to_string),
        yes_price: prices.first().copied().unwrap_or_else(OptionalNumber::unknown),
        no_price: prices.get(1).copied().unwrap_or_else(OptionalNumber::unknown),
        best_bid: num(row.get("bestBid")),
        best_ask: num(row.get("bestAsk")),
        spread: num(row.get("spread")),
        volume: num(row.get("volumeNum")).or_else_num(num(row.get("volume"))),
        liquidity: num(row.get("liquidityNum")).or_else_num(num(row.get("liquidity"))),
        end_date: row
            .get("endDate")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc)),
        resolution_source: empty_to_none(text(row, "resolutionSource")),
        resolution_rules: text(row, "description"),
        resolved_by: text(row, "resolvedBy"),
        active: row.get("active").and_then(Value::as_bool),
        closed: row.get("closed").and_then(Value::as_bool),
        yes_token_id: {
            let ids = parse_json_strings(row.get("clobTokenIds"));
            ids.first().cloned()
        },
        no_token_id: parse_json_strings(row.get("clobTokenIds")).into_iter().nth(1),
        raw: row.clone(),
    })
}

pub fn parse_book(token_id: &str, value: &Value) -> Result<ClobBook, PolymarketError> {
    let bids = levels(value.get("bids"));
    let asks = levels(value.get("asks"));
    let best_bid = bids.first().map(|l| l.0).unwrap_or_else(OptionalNumber::unknown);
    let best_ask = asks.first().map(|l| l.0).unwrap_or_else(OptionalNumber::unknown);
    let mid = match (best_bid.as_f64(), best_ask.as_f64()) {
        (Some(b), Some(a)) => OptionalNumber::observed((a + b) / 2.0),
        _ => OptionalNumber::unknown(),
    };
    let spread = match (best_bid.as_f64(), best_ask.as_f64()) {
        (Some(b), Some(a)) => OptionalNumber::observed(a - b),
        _ => OptionalNumber::unknown(),
    };
    let source_ts = value
        .get("timestamp")
        .and_then(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64()))
        .and_then(DateTime::from_timestamp_millis);
    Ok(ClobBook {
        token_id: token_id.to_string(),
        market: text(value, "market"),
        source_ts,
        best_bid,
        best_ask,
        mid,
        spread,
        bids,
        asks,
    })
}

/// Sharing a parent Gamma event is a LIKELY same-event relation.
/// Similar titles never become PROVEN from this function.
pub fn relations_from_markets(markets: &[PredictionMarket]) -> Vec<MarketRelation> {
    let mut out = Vec::new();
    for (i, left) in markets.iter().enumerate() {
        for right in markets.iter().skip(i + 1) {
            if let (Some(a), Some(b)) = (&left.event_id, &right.event_id) {
                if a == b && left.market_id != right.market_id {
                    out.push(MarketRelation {
                        left_id: left.market_id.clone(),
                        right_id: right.market_id.clone(),
                        relation_type: "same_event".into(),
                        provenance: RelationProvenance::LikelyRelation,
                        evidence: serde_json::json!({
                            "event_id": a,
                            "note": "Gamma parent event id matches; titles are not proof of equivalent contracts"
                        }),
                    });
                }
            }
        }
    }
    out
}

fn levels(value: Option<&Value>) -> Vec<(OptionalNumber, OptionalNumber)> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| (num(row.get("price")), num(row.get("size"))))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_json_array(value: Option<&Value>) -> Vec<OptionalNumber> {
    match value {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .map(|rows| rows.iter().map(|x| num(Some(x))).collect())
            .unwrap_or_default(),
        Some(Value::Array(rows)) => rows.iter().map(|x| num(Some(x))).collect(),
        _ => Vec::new(),
    }
}

fn parse_json_strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
        Some(Value::Array(rows)) => rows
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

fn num(value: Option<&Value>) -> OptionalNumber {
    match value {
        Some(Value::Null) | None => OptionalNumber::unknown(),
        Some(Value::String(s)) => OptionalNumber::from_str(Some(s)),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(OptionalNumber::observed)
            .unwrap_or_else(OptionalNumber::unknown),
        Some(_) => OptionalNumber::unknown(),
    }
}

trait OrElseNum {
    fn or_else_num(self, other: OptionalNumber) -> OptionalNumber;
}

impl OrElseNum for OptionalNumber {
    fn or_else_num(self, other: OptionalNumber) -> OptionalNumber {
        if self.is_unknown() {
            other
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn similar_titles_do_not_create_proven_relations() {
        let markets = [
            PredictionMarket {
                instrument_id: InstrumentId::new(Venue::Polymarket, "1"),
                market_id: "1".into(),
                question: "Xi out before 2027?".into(),
                condition_id: None,
                event_id: Some("e1".into()),
                event_title: None,
                category: None,
                yes_price: OptionalNumber::observed(0.04),
                no_price: OptionalNumber::observed(0.96),
                best_bid: OptionalNumber::unknown(),
                best_ask: OptionalNumber::unknown(),
                spread: OptionalNumber::unknown(),
                volume: OptionalNumber::unknown(),
                liquidity: OptionalNumber::unknown(),
                end_date: None,
                resolution_source: None,
                resolution_rules: None,
                resolved_by: None,
                active: Some(true),
                closed: Some(false),
                yes_token_id: None,
                no_token_id: None,
                raw: json!({}),
            },
            PredictionMarket {
                instrument_id: InstrumentId::new(Venue::Polymarket, "2"),
                market_id: "2".into(),
                question: "Xi Jinping out before 2027?".into(),
                condition_id: None,
                event_id: Some("e2".into()),
                event_title: None,
                category: None,
                yes_price: OptionalNumber::observed(0.05),
                no_price: OptionalNumber::observed(0.95),
                best_bid: OptionalNumber::unknown(),
                best_ask: OptionalNumber::unknown(),
                spread: OptionalNumber::unknown(),
                volume: OptionalNumber::unknown(),
                liquidity: OptionalNumber::unknown(),
                end_date: None,
                resolution_source: None,
                resolution_rules: None,
                resolved_by: None,
                active: Some(true),
                closed: Some(false),
                yes_token_id: None,
                no_token_id: None,
                raw: json!({}),
            },
        ];
        let rels = relations_from_markets(&markets);
        assert!(rels.is_empty());
    }

    #[test]
    fn same_event_id_is_likely_not_proven() {
        let mut a = parse_market(&json!({
            "id":"1","question":"A","outcomePrices":"[\"0.1\",\"0.9\"]","events":[{"id":"9","title":"Evt"}]
        }))
        .unwrap();
        let mut b = a.clone();
        b.market_id = "2".into();
        a.event_id = Some("9".into());
        b.event_id = Some("9".into());
        let rels = relations_from_markets(&[a, b]);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].provenance, RelationProvenance::LikelyRelation);
        assert_ne!(rels[0].provenance, RelationProvenance::ProvenRelation);
    }

    #[test]
    fn empty_resolution_source_is_unknown_not_empty_string() {
        let m = parse_market(&json!({
            "id":"1","question":"Q","resolutionSource":"","outcomePrices":"[\"0.2\",\"0.8\"]"
        }))
        .unwrap();
        assert!(m.resolution_source.is_none());
        assert_eq!(m.yes_price.as_f64(), Some(0.2));
    }
}
