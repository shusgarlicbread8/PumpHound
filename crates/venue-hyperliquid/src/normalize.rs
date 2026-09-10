use chrono::{DateTime, Utc};
use pumphound_venue_core::{InstrumentId, OptionalNumber, Venue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::DEPTH_BUCKETS_USD;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HipClass {
    Native,
    Hip3,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerpDex {
    pub dex: String,
    pub full_name: Option<String>,
    pub deployer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidInstrument {
    pub instrument_id: InstrumentId,
    pub dex: String,
    pub coin: String,
    pub display_name: String,
    pub class: HipClass,
    pub sz_decimals: Option<u32>,
    pub max_leverage: Option<u32>,
    pub delisted: bool,
    pub growth_mode: Option<String>,
    pub deployer_fee_scale: OptionalNumber,
    pub deployer: Option<String>,
    pub mark: OptionalNumber,
    pub mid: OptionalNumber,
    pub oracle: OptionalNumber,
    pub funding: OptionalNumber,
    pub open_interest: OptionalNumber,
    pub premium: OptionalNumber,
    pub day_notional_volume: OptionalNumber,
    pub impact_bid: OptionalNumber,
    pub impact_ask: OptionalNumber,
    pub prev_day_px: OptionalNumber,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookLevel {
    pub px: OptionalNumber,
    pub sz: OptionalNumber,
    pub n: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct L2Book {
    pub coin: String,
    pub source_ts: Option<DateTime<Utc>>,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DepthBucket {
    pub notional_usd: f64,
    pub available: bool,
    pub worst_px: OptionalNumber,
    pub slip_from_mid: OptionalNumber,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentTrade {
    pub coin: String,
    pub px: OptionalNumber,
    pub sz: OptionalNumber,
    pub side: Option<String>,
    pub source_ts: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PredictedFunding {
    pub coin: String,
    pub venue: String,
    pub funding_rate: OptionalNumber,
    pub next_funding_time: Option<DateTime<Utc>>,
    pub interval_hours: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidSnapshot {
    pub instrument: HyperliquidInstrument,
    pub book: Option<L2Book>,
    pub depth: Vec<DepthBucket>,
    pub predicted_hl_funding: OptionalNumber,
    pub predicted_ext_funding: OptionalNumber,
}

pub fn parse_perp_dexs(value: &Value) -> Result<Vec<PerpDex>, String> {
    let Some(arr) = value.as_array() else {
        return Err("perpDexs must be an array".into());
    };
    let mut out = vec![PerpDex {
        dex: String::new(),
        full_name: Some("validator-operated".into()),
        deployer: None,
    }];
    for entry in arr.iter().skip(1) {
        if entry.is_null() {
            continue;
        }
        out.push(PerpDex {
            dex: entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            full_name: entry
                .get("fullName")
                .and_then(Value::as_str)
                .map(str::to_string),
            deployer: entry
                .get("deployer")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(out)
}

pub fn parse_meta_and_ctxs(dex: &str, value: &Value) -> Result<Vec<HyperliquidInstrument>, String> {
    let pair = value
        .as_array()
        .filter(|a| a.len() >= 2)
        .ok_or_else(|| "metaAndAssetCtxs must be [meta, ctxs]".to_string())?;
    let universe = pair[0]
        .get("universe")
        .and_then(Value::as_array)
        .ok_or_else(|| "meta.universe missing".to_string())?;
    let ctxs = pair[1]
        .as_array()
        .ok_or_else(|| "asset ctxs missing".to_string())?;
    if universe.len() != ctxs.len() {
        return Err(format!(
            "universe {} != ctxs {}",
            universe.len(),
            ctxs.len()
        ));
    }
    let class = if dex.is_empty() {
        HipClass::Native
    } else {
        HipClass::Hip3
    };
    let mut out = Vec::with_capacity(universe.len());
    for (asset, ctx) in universe.iter().zip(ctxs) {
        let coin = asset
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if coin.is_empty() {
            continue;
        }
        out.push(HyperliquidInstrument {
            instrument_id: InstrumentId::new(Venue::Hyperliquid, &coin),
            dex: dex.to_string(),
            coin: coin.clone(),
            display_name: coin,
            class,
            sz_decimals: asset.get("szDecimals").and_then(Value::as_u64).map(|v| v as u32),
            max_leverage: asset.get("maxLeverage").and_then(Value::as_u64).map(|v| v as u32),
            delisted: asset.get("isDelisted").and_then(Value::as_bool).unwrap_or(false),
            growth_mode: asset
                .get("growthMode")
                .and_then(Value::as_str)
                .map(str::to_string),
            deployer_fee_scale: OptionalNumber::from_str(
                asset.get("deployerFeeScale").and_then(Value::as_str),
            ),
            deployer: None,
            mark: num(ctx.get("markPx")),
            mid: num(ctx.get("midPx")),
            oracle: num(ctx.get("oraclePx")),
            funding: num(ctx.get("funding")),
            open_interest: num(ctx.get("openInterest")),
            premium: num(ctx.get("premium")),
            day_notional_volume: num(ctx.get("dayNtlVlm")),
            impact_bid: num(
                ctx.get("impactPxs")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first()),
            ),
            impact_ask: num(
                ctx.get("impactPxs")
                    .and_then(Value::as_array)
                    .and_then(|a| a.get(1)),
            ),
            prev_day_px: num(ctx.get("prevDayPx")),
        });
    }
    Ok(out)
}

pub fn parse_l2_book(value: &Value) -> Result<L2Book, String> {
    let coin = value
        .get("coin")
        .and_then(Value::as_str)
        .ok_or_else(|| "l2Book.coin missing".to_string())?
        .to_string();
    let source_ts = value
        .get("time")
        .and_then(Value::as_i64)
        .and_then(ms_to_utc);
    let levels = value
        .get("levels")
        .and_then(Value::as_array)
        .ok_or_else(|| "l2Book.levels missing".to_string())?;
    Ok(L2Book {
        coin,
        source_ts,
        bids: parse_levels(levels.first()),
        asks: parse_levels(levels.get(1)),
    })
}

pub fn parse_recent_trades(value: &Value) -> Result<Vec<RecentTrade>, String> {
    let Some(arr) = value.as_array() else {
        return Err("recentTrades must be an array".into());
    };
    Ok(arr
        .iter()
        .map(|row| RecentTrade {
            coin: row
                .get("coin")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            px: num(row.get("px")),
            sz: num(row.get("sz")),
            side: row.get("side").and_then(Value::as_str).map(str::to_string),
            source_ts: row.get("time").and_then(Value::as_i64).and_then(ms_to_utc),
        })
        .collect())
}

pub fn parse_predicted_fundings(value: &Value) -> Result<Vec<PredictedFunding>, String> {
    let Some(arr) = value.as_array() else {
        return Err("predictedFundings must be an array".into());
    };
    let mut out = Vec::new();
    for row in arr {
        let Some(pair) = row.as_array() else {
            continue;
        };
        let Some(coin) = pair.first().and_then(Value::as_str) else {
            continue;
        };
        let Some(venues) = pair.get(1).and_then(Value::as_array) else {
            continue;
        };
        for venue_row in venues {
            let Some(parts) = venue_row.as_array() else {
                continue;
            };
            let Some(venue) = parts.first().and_then(Value::as_str) else {
                continue;
            };
            let data = parts.get(1);
            out.push(PredictedFunding {
                coin: coin.into(),
                venue: venue.into(),
                funding_rate: num(data.and_then(|v| v.get("fundingRate"))),
                next_funding_time: data
                    .and_then(|v| v.get("nextFundingTime"))
                    .and_then(Value::as_i64)
                    .and_then(ms_to_utc),
                interval_hours: data
                    .and_then(|v| v.get("fundingIntervalHours"))
                    .and_then(Value::as_u64)
                    .map(|v| v as u32),
            });
        }
    }
    Ok(out)
}

pub fn depth_notional(book: &L2Book, mid: OptionalNumber, buy: bool) -> Vec<DepthBucket> {
    let mid = match mid.as_f64() {
        Some(mid) if mid > 0.0 => mid,
        _ => {
            return DEPTH_BUCKETS_USD
                .iter()
                .map(|n| DepthBucket {
                    notional_usd: *n,
                    available: false,
                    worst_px: OptionalNumber::unknown(),
                    slip_from_mid: OptionalNumber::unknown(),
                })
                .collect();
        }
    };
    let levels = if buy { &book.asks } else { &book.bids };
    DEPTH_BUCKETS_USD
        .iter()
        .map(|target| {
            let mut remaining = *target;
            let mut worst = None;
            for level in levels {
                let (Some(px), Some(sz)) = (level.px.as_f64(), level.sz.as_f64()) else {
                    continue;
                };
                let notional = px * sz;
                if notional <= 0.0 {
                    continue;
                }
                remaining -= notional;
                worst = Some(px);
                if remaining <= 0.0 {
                    break;
                }
            }
            if remaining > 0.0 {
                DepthBucket {
                    notional_usd: *target,
                    available: false,
                    worst_px: OptionalNumber::unknown(),
                    slip_from_mid: OptionalNumber::unknown(),
                }
            } else {
                DepthBucket {
                    notional_usd: *target,
                    available: true,
                    worst_px: worst.map(OptionalNumber::observed).unwrap_or_else(OptionalNumber::unknown),
                    slip_from_mid: worst
                        .map(|px| OptionalNumber::observed((px - mid).abs() / mid))
                        .unwrap_or_else(OptionalNumber::unknown),
                }
            }
        })
        .collect()
}

fn parse_levels(value: Option<&Value>) -> Vec<BookLevel> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| BookLevel {
                    px: num(row.get("px")),
                    sz: num(row.get("sz")),
                    n: row.get("n").and_then(Value::as_u64).map(|v| v as u32),
                })
                .collect()
        })
        .unwrap_or_default()
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

fn ms_to_utc(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_live_shaped_meta_and_does_not_zero_missing_mid() {
        let value = json!([
            {"universe":[{"name":"BTC","szDecimals":5,"maxLeverage":40},{"name":"NEW","szDecimals":2,"maxLeverage":5}]},
            [
                {"funding":"0.0000125","openInterest":"1.2","markPx":"100","midPx":"100.5","oraclePx":"99","premium":null,"dayNtlVlm":"10","impactPxs":["99","101"],"prevDayPx":"90","dayBaseVlm":"1"},
                {"funding":"0","openInterest":"0","markPx":"2","midPx":null,"oraclePx":"2","premium":null,"dayNtlVlm":"0","impactPxs":null,"prevDayPx":"2","dayBaseVlm":"0"}
            ]
        ]);
        let rows = parse_meta_and_ctxs("", &value).unwrap();
        assert_eq!(rows[0].class, HipClass::Native);
        assert_eq!(rows[0].mid.as_f64(), Some(100.5));
        assert!(rows[1].mid.is_unknown());
        assert_eq!(rows[1].funding.as_f64(), Some(0.0));
        assert_eq!(rows[1].open_interest.as_f64(), Some(0.0));
    }

    #[test]
    fn hip3_class_from_named_dex() {
        let value = json!([
            {"universe":[{"name":"xyz:NVDA","szDecimals":2,"maxLeverage":10,"growthMode":"enabled"}]},
            [{"funding":"0.0001","openInterest":"3","markPx":"10","midPx":"10","oraclePx":"10","premium":"0","dayNtlVlm":"1","impactPxs":["10","10"],"prevDayPx":"10","dayBaseVlm":"1"}]
        ]);
        let rows = parse_meta_and_ctxs("xyz", &value).unwrap();
        assert_eq!(rows[0].class, HipClass::Hip3);
        assert_eq!(rows[0].growth_mode.as_deref(), Some("enabled"));
    }

    #[test]
    fn insufficient_depth_is_unavailable_not_zero() {
        let book = L2Book {
            coin: "TINY".into(),
            source_ts: None,
            bids: vec![BookLevel {
                px: OptionalNumber::observed(10.0),
                sz: OptionalNumber::observed(1.0),
                n: Some(1),
            }],
            asks: vec![BookLevel {
                px: OptionalNumber::observed(10.1),
                sz: OptionalNumber::observed(1.0),
                n: Some(1),
            }],
        };
        let depth = depth_notional(&book, OptionalNumber::observed(10.05), true);
        assert!(depth.iter().any(|b| b.notional_usd == 1_000.0 && !b.available));
        assert!(depth.iter().all(|b| !b.available));
        assert!(depth[0].worst_px.is_unknown());
    }
}
