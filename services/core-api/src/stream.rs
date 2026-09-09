use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use pumphound_domain::now_utc;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client(socket, state))
}

async fn client(socket: WebSocket, state: Arc<AppState>) {
    let (mut sink, mut src) = socket.split();
    let mut topics = vec!["radar".to_string(), "system".to_string()];
    let mut rx = state.radar_tx.subscribe();
    let mut beat = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            msg = src.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                            if v.get("type").and_then(|x| x.as_str()) == Some("subscribe") {
                                if let Some(arr) = v.get("topics").and_then(|x| x.as_array()) {
                                    topics = arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
                                }
                                // Reports the current shared position without consuming a slot;
                                // see current_seq() for why (a per-connection ack must not steal a
                                // sequence number another connection's delta needs).
                                let seq = state.current_seq().await;
                                let snap = json!({
                                    "type": "snapshot",
                                    "schema_version": 1,
                                    "epoch": state.stream_epoch,
                                    "sequence": seq.to_string(),
                                    "topic": "radar",
                                    "sent_at": now_utc(),
                                    "as_of_slot": null,
                                    "payload": {"subscribed": topics}
                                });
                                if sink.send(Message::Text(snap.to_string().into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = beat.tick() => {
                let seq = state.current_seq().await;
                let h = state.health.read().await;
                let hb = json!({
                    "type": "heartbeat",
                    "schema_version": 1,
                    "epoch": state.stream_epoch,
                    "sequence": seq.to_string(),
                    "topic": "system",
                    "sent_at": now_utc(),
                    "as_of_slot": h.last_slot.to_string(),
                    "payload": {"chain": h.chain, "lag_ms": h.lag_ms}
                });
                if sink.send(Message::Text(hb.to_string().into())).await.is_err() {
                    break;
                }
            }
            ev = rx.recv() => {
                if let Ok(payload) = ev {
                    if topics.iter().any(|t| t == "radar") {
                        // seq was assigned once by the publisher (main.rs) at broadcast time, so
                        // every connection that receives this same event reports the same number.
                        let seq = payload
                            .get("seq")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(state.current_seq().await);
                        let msg = json!({
                            "type": "delta",
                            "schema_version": 1,
                            "epoch": state.stream_epoch,
                            "sequence": seq.to_string(),
                            "topic": "radar",
                            "sent_at": now_utc(),
                            "as_of_slot": null,
                            "payload": payload
                        });
                        if sink.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
}
