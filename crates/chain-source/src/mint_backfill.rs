//! Per-mint early-life backfill. Program-level `getSignaturesForAddress` cannot
//! recover hours of Pump traffic; an individual mint can. Prefer Helius
//! `getTransactionsForAddress` with a tight blockTime window when a key is
//! available. Fall back to a bounded signature walk only when the creation
//! slot is known so high-volume graduated tokens do not page forever.

use crate::websocket::{envelope_from_tx, fetch_transaction, parse_transaction, rpc_result};
use chrono::{DateTime, Utc};
use pumphound_domain::ChainSourceError;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{info, warn};

pub const EARLY_LIFE_SLOT_WINDOW: u64 = 200;
const DEFAULT_MAX_SIGNATURES: usize = 40;
const DEFAULT_MAX_PAGES: usize = 3;
const DEFAULT_THROTTLE_MS: u64 = 400;
const EARLY_LIFE_SECONDS: i64 = 90;

#[derive(Clone, Copy, Debug)]
pub struct MintBackfillCaps {
    pub max_signatures: usize,
    pub max_pages: usize,
    pub fetch_throttle_ms: u64,
}

impl Default for MintBackfillCaps {
    fn default() -> Self {
        Self {
            max_signatures: DEFAULT_MAX_SIGNATURES,
            max_pages: DEFAULT_MAX_PAGES,
            fetch_throttle_ms: DEFAULT_THROTTLE_MS,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SignatureInfo {
    pub signature: String,
    pub slot: u64,
}

pub fn early_window_signatures(
    page: &[SignatureInfo],
    creation_slot: u64,
    slot_window: u64,
) -> (Vec<String>, bool) {
    let mut keep = Vec::new();
    let mut reached_before_create = false;
    for entry in page {
        if entry.slot < creation_slot {
            reached_before_create = true;
            continue;
        }
        if entry.slot > creation_slot.saturating_add(slot_window) {
            continue;
        }
        keep.push(entry.signature.clone());
    }
    (keep, reached_before_create)
}

pub async fn backfill_mint_early_life(
    http: &reqwest::Client,
    rpc: &str,
    mint: &str,
    creation_slot: Option<u64>,
    creation_time: Option<DateTime<Utc>>,
    helius_key_present: bool,
    caps: MintBackfillCaps,
    sink: &mpsc::Sender<pumphound_domain::RawChainEnvelope>,
    generation: u64,
) -> Result<usize, ChainSourceError> {
    if let (Some(created), true) = (creation_time, helius_key_present) {
        match fetch_helius_early_window(http, rpc, mint, created, caps.max_signatures).await {
            Ok(txs) if !txs.is_empty() => {
                let mut sent = 0usize;
                for tx in txs {
                    if sink.send(envelope_from_tx(&tx, generation, None)).await.is_err() {
                        break;
                    }
                    sent += 1;
                }
                info!(mint, recovered = sent, "mint backfill: helius early window");
                return Ok(sent);
            }
            Ok(_) => {}
            Err(error) => warn!(mint, %error, "mint backfill: helius early window failed"),
        }
    }
    let Some(creation_slot) = creation_slot else {
        return Ok(0);
    };
    let mut before: Option<String> = None;
    let mut collected = Vec::new();
    for page in 0..caps.max_pages {
        let mut params = json!({"limit": 1000});
        if let Some(signature) = &before {
            params["before"] = json!(signature);
        }
        let value = match rpc_result(
            http,
            rpc,
            "getSignaturesForAddress",
            json!([mint, params]),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                warn!(mint, %error, "mint backfill getSignaturesForAddress failed");
                break;
            }
        };
        let Some(results) = value.as_array() else {
            break;
        };
        if results.is_empty() {
            break;
        }
        let page_entries: Vec<SignatureInfo> = results
            .iter()
            .filter_map(|entry| {
                Some(SignatureInfo {
                    signature: entry["signature"].as_str()?.to_string(),
                    slot: entry["slot"].as_u64()?,
                })
            })
            .collect();
        let (keep, reached) =
            early_window_signatures(&page_entries, creation_slot, EARLY_LIFE_SLOT_WINDOW);
        collected.extend(keep);
        before = page_entries.last().map(|entry| entry.signature.clone());
        if reached || page_entries.len() < 1000 || collected.len() >= caps.max_signatures {
            let _ = page;
            break;
        }
    }
    collected.truncate(caps.max_signatures);
    collected.reverse();
    let mut sent = 0usize;
    for signature in collected {
        match fetch_transaction(http, rpc, &signature).await {
            Ok(Some(tx)) => {
                if sink.send(envelope_from_tx(&tx, generation, None)).await.is_err() {
                    break;
                }
                sent += 1;
            }
            Ok(None) => {}
            Err(error) => warn!(mint, signature, %error, "mint backfill getTransaction failed"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(caps.fetch_throttle_ms)).await;
    }
    Ok(sent)
}

async fn fetch_helius_early_window(
    http: &reqwest::Client,
    rpc: &str,
    mint: &str,
    created: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<pumphound_domain::RawTransaction>, ChainSourceError> {
    let options = json!({
        "transactionDetails": "full",
        "sortOrder": "asc",
        "limit": limit.min(25),
        "commitment": "finalized",
        "encoding": "json",
        "maxSupportedTransactionVersion": 0,
        "filters": {
            "status": "succeeded",
            "blockTime": {
                "gte": created.timestamp(),
                "lte": created.timestamp().saturating_add(EARLY_LIFE_SECONDS)
            }
        }
    });
    let value = rpc_result(
        http,
        rpc,
        "getTransactionsForAddress",
        json!([mint, options]),
    )
    .await?;
    let Some(data) = value
        .pointer("/data")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for row in data {
        let Some(signature) = row
            .pointer("/transaction/signatures/0")
            .and_then(Value::as_str)
        else {
            continue;
        };
        out.push(parse_transaction(signature, &row));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_early_slot_window_and_stops_before_create() {
        let page = vec![
            SignatureInfo {
                signature: "late".into(),
                slot: 500,
            },
            SignatureInfo {
                signature: "early".into(),
                slot: 105,
            },
            SignatureInfo {
                signature: "create".into(),
                slot: 100,
            },
            SignatureInfo {
                signature: "before".into(),
                slot: 90,
            },
        ];
        let (kept, reached) = early_window_signatures(&page, 100, 50);
        assert_eq!(kept, vec!["early".to_string(), "create".to_string()]);
        assert!(reached);
    }
}
