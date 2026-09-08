use crate::ChainSource;
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use pumphound_domain::{
    payload_sha256, sighting_id, ChainCursor, ChainSourceError, Commitment, RawChainEnvelope,
    RawKind, Slot, TransportMeta,
};
use serde_json::Value;
use tokio::sync::mpsc;

pub struct ParquetReplaySource {
    path: String,
    speed: f64,
}

impl ParquetReplaySource {
    pub fn new(path: impl Into<String>, speed: f64) -> Self {
        Self {
            path: path.into(),
            speed,
        }
    }
}

#[async_trait]
impl ChainSource for ParquetReplaySource {
    async fn run(
        &mut self,
        _cursor: Option<ChainCursor>,
        sink: mpsc::Sender<RawChainEnvelope>,
    ) -> Result<(), ChainSourceError> {
        let text = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| ChainSourceError::Io(e.to_string()))?;
        let rows = load_rows(&self.path, &text)?;
        let delay = if self.speed <= 0.0 {
            0
        } else {
            (50.0 / self.speed) as u64
        };
        for (i, payload) in rows.into_iter().enumerate() {
            let slot = parse_slot(&payload).unwrap_or(Slot(i as u64));
            let observed_at = parse_time(&payload, "block_time")
                .or_else(|| parse_time(&payload, "observed_at"))
                .unwrap_or_else(Utc::now);
            let hash = payload_sha256(payload.to_string().as_bytes());
            let env = RawChainEnvelope {
                sighting_id: sighting_id("replay", "1", &i.to_string(), &hash),
                source: "replay".into(),
                source_session: "1".into(),
                source_cursor: i.to_string(),
                slot,
                blockhash: None,
                parent_slot: None,
                commitment: Commitment::Finalized,
                observed_at,
                payload_sha256: hash,
                kind: RawKind::Transaction,
                payload,
                transport: TransportMeta {
                    source_lag_ms: Some(0),
                    received_bytes: 0,
                    reconnect_generation: 0,
                },
            };
            if sink.send(env).await.is_err() {
                break;
            }
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }
        Ok(())
    }
}

fn load_rows(path: &str, text: &str) -> Result<Vec<Value>, ChainSourceError> {
    if path.ends_with(".jsonl") {
        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            out.push(
                serde_json::from_str(line)
                    .map_err(|e| ChainSourceError::Transport(format!("jsonl line {n}: {e}")))?,
            );
        }
        return Ok(out);
    }
    if path.ends_with(".json") {
        let v: Value =
            serde_json::from_str(text).map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        if let Some(arr) = v.as_array() {
            return Ok(arr.clone());
        }
        if let Some(arr) = v.get("transactions").and_then(|a| a.as_array()) {
            return Ok(arr.clone());
        }
        return Err(ChainSourceError::Transport(
            "replay JSON must be an array of RawTransaction objects".into(),
        ));
    }
    Err(ChainSourceError::Config(
        "replay source accepts .json or .jsonl fixtures in v1".into(),
    ))
}

fn parse_slot(payload: &Value) -> Option<Slot> {
    payload
        .get("slot")
        .and_then(|s| {
            s.as_u64()
                .or_else(|| s.as_str().and_then(|t| t.parse().ok()))
        })
        .map(Slot)
}

fn parse_time(payload: &Value, key: &str) -> Option<DateTime<Utc>> {
    let v = payload.get(key)?;
    if let Some(n) = v.as_i64() {
        return Utc.timestamp_opt(n, 0).single();
    }
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_accepts_decimal_string() {
        let v = serde_json::json!({"slot": "350000001"});
        assert_eq!(parse_slot(&v).unwrap().0, 350_000_001);
    }
}
