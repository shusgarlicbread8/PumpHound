use crate::{ChainSource, SourceConfig};
use async_trait::async_trait;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use pumphound_domain::{
    chain_time_from_parts, payload_sha256, sighting_id, ChainCursor, ChainSourceError, Commitment,
    RawChainEnvelope, RawInnerInstructions, RawInstruction, RawKind, RawTransaction, Slot,
    TransportMeta,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

pub struct SolanaWebsocketSource {
    cfg: SourceConfig,
    generation: u64,
}

impl SolanaWebsocketSource {
    pub fn new(cfg: SourceConfig) -> Self {
        Self { cfg, generation: 0 }
    }
}

#[async_trait]
impl ChainSource for SolanaWebsocketSource {
    async fn run(
        &mut self,
        cursor: Option<ChainCursor>,
        sink: mpsc::Sender<RawChainEnvelope>,
    ) -> Result<(), ChainSourceError> {
        let mut backoff_ms = 500u64;
        loop {
            self.generation += 1;
            match self.session(cursor.as_ref(), &sink).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    warn!(error = %e, generation = self.generation, "websocket session ended");
                    if sink.is_closed() {
                        return Err(e);
                    }
                    let jitter = (self.generation.wrapping_mul(17)) % 250;
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms + jitter)).await;
                    backoff_ms = (backoff_ms * 2).min(15_000);
                }
            }
        }
    }
}

impl SolanaWebsocketSource {
    async fn session(
        &self,
        _cursor: Option<&ChainCursor>,
        sink: &mpsc::Sender<RawChainEnvelope>,
    ) -> Result<(), ChainSourceError> {
        let ws_host = self
            .cfg
            .ws_url
            .split('?')
            .next()
            .unwrap_or("wss://redacted");
        info!(url = %ws_host, "connecting Solana logsSubscribe");
        let (ws, _) = connect_async(&self.cfg.ws_url)
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        let (mut write, mut read) = ws.split();
        let mut req_id = 1u64;
        for program in &self.cfg.program_ids {
            let msg = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "logsSubscribe",
                "params": [
                    {"mentions": [program]},
                    {"commitment": "processed"}
                ]
            });
            write
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    msg.to_string().into(),
                ))
                .await
                .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
            req_id += 1;
        }
        let slot_sub = json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "slotSubscribe"
        });
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                slot_sub.to_string().into(),
            ))
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;

        let fetch_full_transactions = std::env::var("PUMPHOUND_LIVE_FETCH_TRANSACTIONS")
            .ok()
            .as_deref()
            == Some("1");
        let hot_fetch = std::env::var("PUMPHOUND_HOT_SET_FETCH").ok().as_deref() != Some("0");
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        if !fetch_full_transactions {
            info!(
                hot_fetch,
                "broad scout uses logsSubscribe; getTransaction only for hot-set mints"
            );
        }

        loop {
            let next = tokio::time::timeout(std::time::Duration::from_secs(20), read.next()).await;
            let msg = match next {
                Ok(Some(msg)) => msg.map_err(|e| ChainSourceError::Transport(e.to_string()))?,
                Ok(None) => return Err(ChainSourceError::Disconnected("stream ended".into())),
                Err(_) => {
                    return Err(ChainSourceError::Disconnected(
                        "idle timeout waiting for logs/slot".into(),
                    ));
                }
            };
            let text = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
                tokio_tungstenite::tungstenite::Message::Ping(p) => {
                    let _ = write
                        .send(tokio_tungstenite::tungstenite::Message::Pong(p))
                        .await;
                    continue;
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => {
                    return Err(ChainSourceError::Disconnected("server close".into()));
                }
                _ => continue,
            };
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
            if let Some(params) = v.get("params") {
                if let Some(sig) = params
                    .pointer("/result/value/signature")
                    .and_then(|s| s.as_str())
                {
                    let err = params.pointer("/result/value/err").cloned();
                    let logs = params
                        .pointer("/result/value/logs")
                        .and_then(|value| value.as_array())
                        .map(|lines| {
                            lines
                                .iter()
                                .filter_map(|line| line.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let fetch_this = fetch_full_transactions
                        || crate::full_tx::logs_warrant_full_transaction(&logs)
                        || (hot_fetch && crate::hot_mints::logs_mention_hot(&logs));
                    if fetch_this {
                        match fetch_transaction(&http, &self.cfg.rpc_url, sig).await {
                            Ok(Some(raw_tx)) => {
                                let env = envelope_from_tx(&raw_tx, self.generation, err);
                                if sink.send(env).await.is_err() {
                                    return Ok(());
                                }
                            }
                            Ok(None) => {
                                if let Some(raw_tx) = transaction_from_logs_notification(params) {
                                    let env = envelope_from_tx(&raw_tx, self.generation, err);
                                    if sink.send(env).await.is_err() {
                                        return Ok(());
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(signature = sig, error = %e, "getTransaction failed");
                                if let Some(raw_tx) = transaction_from_logs_notification(params) {
                                    let env = envelope_from_tx(&raw_tx, self.generation, err);
                                    if sink.send(env).await.is_err() {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    } else if let Some(raw_tx) = transaction_from_logs_notification(params) {
                        let env = envelope_from_tx(&raw_tx, self.generation, err);
                        if sink.send(env).await.is_err() {
                            return Ok(());
                        }
                    }
                } else if let Some(slot) = params.pointer("/result/slot").and_then(|s| s.as_u64()) {
                    let now = Utc::now();
                    let payload = json!({"slot": slot});
                    let hash = payload_sha256(payload.to_string().as_bytes());
                    let env = RawChainEnvelope {
                        sighting_id: sighting_id(
                            "solana-ws",
                            &self.generation.to_string(),
                            &slot.to_string(),
                            &hash,
                        ),
                        source: "solana-ws".into(),
                        source_session: self.generation.to_string(),
                        source_cursor: slot.to_string(),
                        slot: Slot(slot),
                        blockhash: None,
                        parent_slot: None,
                        commitment: Commitment::Processed,
                        observed_at: now,
                        payload_sha256: hash,
                        kind: RawKind::Slot,
                        payload,
                        transport: TransportMeta {
                            source_lag_ms: None,
                            received_bytes: text.len() as u64,
                            reconnect_generation: self.generation,
                        },
                    };
                    if sink.send(env).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

pub fn transaction_from_logs_notification(params: &Value) -> Option<RawTransaction> {
    let value = params.pointer("/result/value")?;
    let signature = value.get("signature")?.as_str()?.to_string();
    let slot = params.pointer("/result/context/slot")?.as_u64()?;
    let log_messages: Vec<String> = value
        .get("logs")?
        .as_array()?
        .iter()
        .filter_map(|line| line.as_str().map(str::to_string))
        .collect();
    let mut account_keys: Vec<String> = log_messages
        .iter()
        .filter_map(|line| {
            line.strip_prefix("Program ")
                .and_then(|rest| rest.split_whitespace().next())
                .filter(|_| line.contains(" invoke ["))
                .map(str::to_string)
        })
        .collect();
    account_keys.sort();
    account_keys.dedup();
    Some(RawTransaction {
        signature,
        slot: Slot(slot),
        block_time: None,
        transaction_index: None,
        err: value.get("err").cloned().filter(|error| !error.is_null()),
        fee_payer: None,
        account_keys,
        loaded_writable_addresses: vec![],
        loaded_readonly_addresses: vec![],
        instructions: vec![],
        inner_instructions: vec![],
        log_messages,
        pre_balances: vec![],
        post_balances: vec![],
        pre_token_balances: vec![],
        post_token_balances: vec![],
        compute_units_consumed: None,
        raw_json: params.clone(),
    })
}

/// Compact `json` first. `jsonParsed` responses are often multi-megabyte and
/// get truncated or rejected by the HTTP JSON decoder.
pub async fn fetch_transaction(
    http: &reqwest::Client,
    rpc: &str,
    signature: &str,
) -> Result<Option<RawTransaction>, ChainSourceError> {
    match fetch_transaction_encoded(http, rpc, signature, "json").await {
        Ok(tx) => Ok(tx),
        Err(first) => match fetch_transaction_encoded(http, rpc, signature, "jsonParsed").await {
            Ok(tx) => Ok(tx),
            Err(_) => Err(first),
        },
    }
}

async fn fetch_transaction_encoded(
    http: &reqwest::Client,
    rpc: &str,
    signature: &str,
    encoding: &str,
) -> Result<Option<RawTransaction>, ChainSourceError> {
    let result = rpc_result(
        http,
        rpc,
        "getTransaction",
        json!([
            signature,
            {
                "encoding": encoding,
                "maxSupportedTransactionVersion": 0,
                "commitment": "confirmed"
            }
        ]),
    )
    .await?;
    if result.is_null() {
        return Ok(None);
    }
    Ok(Some(parse_transaction(signature, &result)))
}

pub async fn fetch_block_time(
    http: &reqwest::Client,
    rpc: &str,
    slot: u64,
) -> Result<Option<chrono::DateTime<Utc>>, ChainSourceError> {
    let result = rpc_result(http, rpc, "getBlockTime", json!([slot])).await?;
    Ok(block_time_from_rpc(&result))
}

pub async fn fetch_block_times(
    http: &reqwest::Client,
    rpc: &str,
    slots: &[u64],
) -> Result<HashMap<u64, chrono::DateTime<Utc>>, ChainSourceError> {
    let mut out = HashMap::new();
    let chunks = slots.chunks(25).collect::<Vec<_>>();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let batch: Vec<Value> = chunk
            .iter()
            .map(|slot| {
                json!({
                    "jsonrpc": "2.0",
                    "id": slot,
                    "method": "getBlockTime",
                    "params": [slot]
                })
            })
            .collect();
        let bytes = rpc_post(http, rpc, &Value::Array(batch)).await?;
        let values: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| {
            ChainSourceError::Transport(format!("invalid JSON ({e}): {}", body_preview(&bytes)))
        })?;
        for value in values {
            if value.get("error").is_some() {
                continue;
            }
            let Some(slot) = value.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if let Some(time) = block_time_from_rpc(value.get("result").unwrap_or(&Value::Null)) {
                out.insert(slot, time);
            }
        }
        if (index + 1) % 20 == 0 {
            info!(
                done = index + 1,
                resolved = out.len(),
                "getBlockTime batch progress"
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    Ok(out)
}

pub async fn rpc_result(
    http: &reqwest::Client,
    rpc: &str,
    method: &str,
    params: Value,
) -> Result<Value, ChainSourceError> {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let bytes = rpc_post(http, rpc, &body).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| {
        ChainSourceError::Transport(format!("invalid JSON ({e}): {}", body_preview(&bytes)))
    })?;
    if let Some(error) = value.get("error") {
        return Err(ChainSourceError::Transport(format!(
            "JSON-RPC error: {error}"
        )));
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

async fn rpc_post(
    http: &reqwest::Client,
    rpc: &str,
    body: &Value,
) -> Result<Vec<u8>, ChainSourceError> {
    let mut last = String::new();
    for attempt in 0..6 {
        let response = http
            .post(rpc)
            .json(body)
            .send()
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        if status.as_u16() == 429 || status.is_server_error() {
            last = format!("HTTP {status}: {}", body_preview(&bytes));
            let delay = 1_000u64.saturating_mul(1u64 << attempt.min(4)).min(20_000);
            warn!(attempt, delay_ms = delay, %last, "RPC backing off");
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            continue;
        }
        if !status.is_success() {
            return Err(ChainSourceError::Transport(format!(
                "HTTP {status}: {}",
                body_preview(&bytes)
            )));
        }
        return Ok(bytes.to_vec());
    }
    Err(ChainSourceError::Transport(last))
}

fn block_time_from_rpc(result: &Value) -> Option<chrono::DateTime<Utc>> {
    let ts = result.as_i64()?;
    chain_time_from_parts(chrono::DateTime::from_timestamp(ts, 0), None)
}

fn body_preview(bytes: &[u8]) -> String {
    const LIMIT: usize = 180;
    let end = bytes.len().min(LIMIT);
    String::from_utf8_lossy(&bytes[..end]).replace('\n', " ")
}

pub fn parse_transaction(signature: &str, result: &Value) -> RawTransaction {
    let slot = result.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);
    let block_time = result
        .get("blockTime")
        .and_then(|t| t.as_i64())
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .and_then(|t| chain_time_from_parts(Some(t), None));
    let meta = result.get("meta").cloned().unwrap_or(json!({}));
    let msg = result
        .pointer("/transaction/message")
        .cloned()
        .unwrap_or(json!({}));
    let mut account_keys = Vec::new();
    if let Some(keys) = msg.get("accountKeys").and_then(|k| k.as_array()) {
        for k in keys {
            if let Some(s) = k.as_str() {
                account_keys.push(s.to_string());
            } else if let Some(s) = k.get("pubkey").and_then(|p| p.as_str()) {
                account_keys.push(s.to_string());
            }
        }
    }
    let loaded_w: Vec<String> = meta
        .pointer("/loadedAddresses/writable")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let loaded_r: Vec<String> = meta
        .pointer("/loadedAddresses/readonly")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut resolved_keys = account_keys.clone();
    resolved_keys.extend(loaded_w.iter().cloned());
    resolved_keys.extend(loaded_r.iter().cloned());
    let instructions = parse_ixs(msg.get("instructions"), &resolved_keys);
    let inner = meta
        .get("innerInstructions")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .map(|ii| RawInnerInstructions {
                    index: ii.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
                    instructions: parse_ixs(ii.get("instructions"), &resolved_keys),
                })
                .collect()
        })
        .unwrap_or_default();
    let logs = meta
        .get("logMessages")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    RawTransaction {
        signature: signature.to_string(),
        slot: Slot(slot),
        block_time,
        transaction_index: result
            .get("transactionIndex")
            .or_else(|| result.get("index"))
            .and_then(|i| i.as_u64())
            .map(|i| i as u32),
        err: meta.get("err").cloned().filter(|e| !e.is_null()),
        fee_payer: account_keys.first().cloned(),
        account_keys,
        loaded_writable_addresses: loaded_w,
        loaded_readonly_addresses: loaded_r,
        instructions,
        inner_instructions: inner,
        log_messages: logs,
        pre_balances: meta
            .get("preBalances")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default(),
        post_balances: meta
            .get("postBalances")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default(),
        pre_token_balances: meta
            .get("preTokenBalances")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default(),
        post_token_balances: meta
            .get("postTokenBalances")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default(),
        compute_units_consumed: meta.get("computeUnitsConsumed").and_then(|v| v.as_u64()),
        raw_json: result.clone(),
    }
}

fn parse_ixs(v: Option<&Value>, account_keys: &[String]) -> Vec<RawInstruction> {
    let Some(arr) = v.and_then(|a| a.as_array()) else {
        return vec![];
    };
    arr.iter()
        .map(|ix| {
            let program_id = ix
                .get("programId")
                .and_then(|p| p.as_str())
                .or_else(|| ix.pointer("/programId").and_then(|p| p.as_str()))
                .map(str::to_string)
                .or_else(|| {
                    ix.get("programIdIndex")
                        .and_then(|index| index.as_u64())
                        .and_then(|index| account_keys.get(index as usize))
                        .cloned()
                })
                .unwrap_or_default();
            let accounts = ix
                .get("accounts")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| {
                                    v.get("pubkey")
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string())
                                })
                                .or_else(|| {
                                    v.as_u64()
                                        .and_then(|index| account_keys.get(index as usize))
                                        .cloned()
                                })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let data = ix.get("data").and_then(|d| d.as_str()).unwrap_or("");
            RawInstruction {
                program_id,
                accounts,
                data_base64: data.to_string(),
                stack_height: ix
                    .get("stackHeight")
                    .and_then(|s| s.as_u64())
                    .map(|s| s as u32),
            }
        })
        .collect()
}

pub fn envelope_from_tx(
    tx: &RawTransaction,
    generation: u64,
    logs_err: Option<Value>,
) -> RawChainEnvelope {
    let mut payload = serde_json::to_value(tx).unwrap_or(json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("signature".into(), json!(tx.signature));
        if let Some(e) = logs_err {
            obj.insert("logs_err".into(), e);
        }
    }
    let hash = payload_sha256(payload.to_string().as_bytes());
    RawChainEnvelope {
        sighting_id: sighting_id("solana-ws", &generation.to_string(), &tx.signature, &hash),
        source: "solana-ws".into(),
        source_session: generation.to_string(),
        source_cursor: tx.signature.clone(),
        slot: tx.slot,
        blockhash: None,
        parent_slot: None,
        commitment: Commitment::Confirmed,
        observed_at: Utc::now(),
        payload_sha256: hash,
        kind: RawKind::Transaction,
        payload,
        transport: TransportMeta {
            source_lag_ms: tx.block_time.map(|bt| {
                let lag = (Utc::now() - bt).num_milliseconds();
                lag.max(0)
            }),
            received_bytes: 0,
            reconnect_generation: generation,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_notification_becomes_raw_transaction_without_rpc_fetch() {
        let params = json!({
            "result": {
                "context": {"slot": 42},
                "value": {
                    "signature": "sig",
                    "err": null,
                    "logs": [
                        format!("Program {} invoke [1]", pumphound_domain::PUMP_PROGRAM_ID),
                        "Program data: AAAAAAAAAAA=",
                        format!("Program {} success", pumphound_domain::PUMP_PROGRAM_ID)
                    ]
                }
            }
        });
        let tx = transaction_from_logs_notification(&params).expect("valid logs notification");
        assert_eq!(tx.signature, "sig");
        assert_eq!(tx.slot, Slot(42));
        assert_eq!(
            tx.account_keys,
            vec![pumphound_domain::PUMP_PROGRAM_ID.to_string()]
        );
        assert_eq!(tx.log_messages.len(), 3);
        assert!(tx.block_time.is_none());
        assert!(tx.instructions.is_empty());
    }

    #[test]
    fn parse_transaction_drops_impossible_block_times() {
        let result = json!({
            "slot": 100,
            "blockTime": 10_409_356_800i64,
            "transaction": {"message": {"accountKeys": ["11111111111111111111111111111111"]}},
            "meta": {"err": null, "logMessages": []}
        });
        let tx = parse_transaction("sig", &result);
        assert!(tx.block_time.is_none());
        assert_eq!(tx.slot, Slot(100));
    }
}
