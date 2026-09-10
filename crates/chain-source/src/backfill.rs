//! Fills the gap between a source's last durable checkpoint and "now" using RPC history
//! (`getSignaturesForAddress` + `getTransaction`), so a restart after a brief outage — a crash,
//! a deploy, a panic-and-restart — catches up on what was missed instead of silently resuming
//! live tailing from wherever the stream happens to be. `logsSubscribe` (the live path,
//! `websocket.rs`) is a push-only subscription with no replay of its own; before this module,
//! `ChainCursor` was written durably on every processed sighting and read back at startup, but
//! never actually consumed — `session()`'s cursor parameter was `_cursor`, unused. Any downtime
//! was a silent, permanent gap with no attempt to recover it and no record that it happened.
//!
//! Feeds the SAME channel and decode/persist/journal pipeline live transactions use
//! (`envelope_from_tx` + `fetch_transaction`, the exact functions the live hot-upgrade path
//! already calls), so backfilled and live sightings are indistinguishable downstream. Call this
//! concurrently with (not before) starting the live source: any overlap between backfill's tail
//! and live's head is deduplicated by the existing sighting_id / `ON CONFLICT DO NOTHING` path,
//! the same as any other duplicate delivery this pipeline already tolerates. Preferring a little
//! duplicate work over a timing gap is the safe direction to bias.
//!
//! Stops on a *strictly older* slot, not on `until` matching a literal signature:
//! `getSignaturesForAddress`'s `until` only terminates cleanly when that exact signature appears
//! in the given address's own history, which a single shared last-processed-signature cursor
//! across multiple program IDs cannot guarantee. The checkpoint is the last *processed*
//! transaction's slot, not proof that every other transaction in that slot was seen — a slot on
//! Solana holds many txs. Stopping at the first `slot <= checkpoint` permanently dropped the
//! remaining same-slot signatures on that page. Keep every signature with `slot > checkpoint`
//! and every sibling still in the checkpoint slot (the live pipeline already deduplicates);
//! only treat `slot < checkpoint` as "walked past the gap." The exact checkpoint signature, when
//! known, is skipped as already processed.
//!
//! **Measured limit, not a guess (2026-09-09, mainnet):** the pump.fun program address is
//! mentioned in roughly 250 transactions per 11 slots — about 55-60 tx/s network-wide, almost
//! entirely unrelated to any specific mint PumpHound tracks. The first version of this module
//! used a 50-page/50,000-signature cap "to be generous"; on the very first live restart it hit
//! that cap without reaching a checkpoint only ~5 seconds old, and kept trying to fetch full
//! transaction detail for all 50,000 of them — a ~50 minute RPC-hammering run for one program,
//! caught and killed manually before it could do that to a second. Given that real throughput,
//! per-program signature enumeration can only ever recover on the order of tens of seconds to
//! roughly a minute of gap before the RPC-call volume becomes impractical; it is a safety net for
//! a crash-restart or a deploy, not a way to recover from the collector being down for hours.
//! Reflect that in the caps below rather than picking a number that "sounds" generous — a cap
//! that can't be exhausted safely within a few seconds is a resource-exhaustion bug against a
//! live provider, not a value-add. Recovering longer gaps on already-tracked mints specifically
//! (much lower per-mint volume than the shared program address) is a real, separate follow-up;
//! it is not implemented here.
//!
//! **Revised down again (2026-09-09, later same day):** at 3 pages/3,000 signatures, every
//! observed restart — not just long-outage ones — hit this cap outright, meaning the "safety net"
//! was unconditionally spending its full worst-case budget (3,000 signatures x
//! `FETCH_THROTTLE_MS` = up to 5 minutes of sustained extra writes) on every restart regardless of
//! how brief the actual gap was, stacking with the periodic intelligence refresh
//! (`services/collector/src/main.rs`'s `refresh_intelligence` loop) to push `/api/v1/home` past
//! 15s for the whole window. Real pump_curve throughput has evidently grown past the original
//! 55-60 tx/s measurement. Cut to 1 page/1,000 signatures so the worst case drops to roughly a
//! third of that, closer to the "brief restart" budget this was meant to be.
const MAX_PAGES_PER_PROGRAM: usize = 1;
const PAGE_LIMIT: usize = 1000;
/// Between individual getTransaction calls during catch-up. Two things to protect against, not
/// just the RPC provider's rate limit: a live run on 2026-09-09 showed each backfilled sighting
/// pushes through the exact same decode/persist writes a live one does (ingestion_batches,
/// canonical_event_keys, graph_facts, raw_archive_index, ...), and even the ~1 req/50ms this
/// constant used to allow was enough concurrent write pressure to push ordinary single-row
/// Postgres upserts on the *live* path past the 1s slow-statement threshold, and `/api/v1/home`
/// briefly past 5s, for the run's whole duration. 100ms trades a slower catch-up (a bounded,
/// occasional, restart-only cost) for leaving live serving alone — the right side of that trade,
/// since backfill exists to protect data no one is looking at yet, not to compete with someone
/// looking at the app right now.
const FETCH_THROTTLE_MS: u64 = 100;

use pumphound_domain::{ChainCursor, ChainSourceError, Slot};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::websocket::{envelope_from_tx, fetch_transaction};

pub async fn backfill_since_checkpoint(
    http: &reqwest::Client,
    rpc: &str,
    program_ids: &[String],
    checkpoint: &ChainCursor,
    sink: &mpsc::Sender<pumphound_domain::RawChainEnvelope>,
    generation: u64,
) -> usize {
    let Some(checkpoint_slot) = checkpoint.slot else {
        info!("backfill: checkpoint has no recorded slot; nothing to catch up from");
        return 0;
    };
    info!(
        checkpoint_slot = checkpoint_slot.0,
        programs = program_ids.len(),
        "backfill: catching up from last checkpoint before/alongside live tailing"
    );
    let mut total = 0usize;
    for program in program_ids {
        match backfill_one_program(
            http,
            rpc,
            program,
            checkpoint_slot,
            checkpoint.signature.as_deref(),
            sink,
            generation,
        )
        .await
        {
            Ok(n) => total += n,
            Err(e) => warn!(
                program,
                error = %e,
                "backfill for this program failed; live tailing proceeds regardless — this gap \
                 was not filled and is not otherwise recorded, re-run will retry from the same \
                 checkpoint"
            ),
        }
    }
    info!(sightings_recovered = total, "backfill: caught up");
    total
}

async fn backfill_one_program(
    http: &reqwest::Client,
    rpc: &str,
    program: &str,
    checkpoint_slot: Slot,
    checkpoint_signature: Option<&str>,
    sink: &mpsc::Sender<pumphound_domain::RawChainEnvelope>,
    generation: u64,
) -> Result<usize, ChainSourceError> {
    let mut before: Option<String> = None;
    // Collected newest-first (RPC's native order); reversed once, below, before replay so
    // downstream sees history in chronological order like the live path does.
    let mut collected: Vec<String> = Vec::new();
    let mut reached_checkpoint = false;
    for page in 0..MAX_PAGES_PER_PROGRAM {
        let mut params = json!({"limit": PAGE_LIMIT});
        if let Some(b) = &before {
            params["before"] = json!(b);
        }
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
            "params": [program, params]
        });
        let response = http
            .post(rpc)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        let value: Value = response
            .json()
            .await
            .map_err(|e| ChainSourceError::Transport(e.to_string()))?;
        let Some(results) = value["result"].as_array() else {
            break;
        };
        if results.is_empty() {
            break;
        }
        let page_len = results.len();
        let (page_signatures, page_reached_checkpoint) = signatures_newer_than_checkpoint(
            results,
            checkpoint_slot.0,
            checkpoint_signature,
        );
        collected.extend(page_signatures);
        reached_checkpoint = page_reached_checkpoint;
        before = results
            .last()
            .and_then(|e| e["signature"].as_str())
            .map(str::to_string);
        if reached_checkpoint || page_len < PAGE_LIMIT {
            break;
        }
        if page == MAX_PAGES_PER_PROGRAM - 1 {
            warn!(
                program,
                pages = MAX_PAGES_PER_PROGRAM,
                signatures_found = collected.len(),
                "backfill hit its page safety cap before reaching the checkpoint slot; downtime \
                 was likely longer than one pass can safely cover — the remaining, older gap is \
                 NOT filled this run"
            );
        }
    }
    info!(
        program,
        count = collected.len(),
        reached_checkpoint,
        "backfill: signatures found since checkpoint for this program"
    );
    collected.reverse();
    let mut sent = 0usize;
    for sig in collected {
        match fetch_transaction(http, rpc, &sig).await {
            Ok(Some(raw_tx)) => {
                let env = envelope_from_tx(&raw_tx, generation, None);
                if sink.send(env).await.is_err() {
                    break;
                }
                sent += 1;
            }
            Ok(None) => {}
            Err(e) => warn!(
                signature = sig,
                error = %e,
                "backfill getTransaction failed for one signature; continuing with the rest \
                 (this specific sighting stays missing until a future backfill run retries it)"
            ),
        }
        tokio::time::sleep(std::time::Duration::from_millis(FETCH_THROTTLE_MS)).await;
    }
    Ok(sent)
}

/// Pure: which signatures in one `getSignaturesForAddress` page still belong in the gap, and
/// whether pagination should stop. Newest-first RPC order. A slot has many transactions; the
/// checkpoint names the last processed one, not the end of that slot. Keep `slot > checkpoint`
/// and remaining same-slot siblings. Only `slot < checkpoint` means we have walked past the gap.
/// The exact checkpoint signature, when known, is already durable and is not re-fetched.
fn signatures_newer_than_checkpoint(
    results: &[Value],
    checkpoint_slot: u64,
    checkpoint_signature: Option<&str>,
) -> (Vec<String>, bool) {
    let mut keep = Vec::new();
    for entry in results {
        let Some(sig) = entry["signature"].as_str() else {
            continue;
        };
        let slot = entry["slot"].as_u64().unwrap_or(0);
        if slot < checkpoint_slot {
            return (keep, true);
        }
        if slot == checkpoint_slot && checkpoint_signature == Some(sig) {
            continue;
        }
        keep.push(sig.to_string());
    }
    (keep, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sig: &str, slot: u64) -> Value {
        json!({"signature": sig, "slot": slot})
    }

    #[test]
    fn keeps_everything_strictly_newer_than_the_checkpoint() {
        let page = vec![entry("c", 30), entry("b", 20), entry("a", 10)];
        let (kept, reached) = signatures_newer_than_checkpoint(&page, 5, None);
        assert_eq!(kept, vec!["c", "b", "a"]);
        assert!(!reached, "checkpoint slot 5 is older than every entry; page never reached it");
    }

    #[test]
    fn keeps_same_slot_siblings_and_stops_only_on_an_older_slot() {
        // Newest-first page: two more txs in the checkpoint slot after the first one we see.
        // The old `slot <= checkpoint` early-return dropped `same_b` and `same_c` permanently.
        let page = vec![
            entry("newer", 30),
            entry("same_a", 20),
            entry("same_b", 20),
            entry("same_c", 20),
            entry("older", 19),
        ];
        let (kept, reached) = signatures_newer_than_checkpoint(&page, 20, None);
        assert_eq!(kept, vec!["newer", "same_a", "same_b", "same_c"]);
        assert!(reached);
    }

    #[test]
    fn skips_the_exact_checkpoint_signature_but_keeps_other_same_slot_txs() {
        let page = vec![
            entry("newer", 21),
            entry("later_in_slot", 20),
            entry("already_processed", 20),
            entry("earlier_in_slot", 20),
            entry("older", 19),
        ];
        let (kept, reached) =
            signatures_newer_than_checkpoint(&page, 20, Some("already_processed"));
        assert_eq!(kept, vec!["newer", "later_in_slot", "earlier_in_slot"]);
        assert!(reached);
    }

    #[test]
    fn stops_below_the_checkpoint_slot_too() {
        let page = vec![entry("newer", 30), entry("older_than_checkpoint", 4)];
        let (kept, reached) = signatures_newer_than_checkpoint(&page, 5, None);
        assert_eq!(kept, vec!["newer"]);
        assert!(reached);
    }

    #[test]
    fn empty_page_is_not_treated_as_reaching_the_checkpoint() {
        let (kept, reached) = signatures_newer_than_checkpoint(&[], 5, None);
        assert!(kept.is_empty());
        assert!(!reached, "an empty page must be handled by the caller's own empty-page check, not misread as ​\"caught up\"");
    }

    #[test]
    fn entry_missing_a_signature_field_is_skipped_not_fatal() {
        let page = vec![json!({"slot": 30}), entry("has_sig", 25)];
        let (kept, reached) = signatures_newer_than_checkpoint(&page, 5, None);
        assert_eq!(kept, vec!["has_sig"]);
        assert!(!reached);
    }
}
