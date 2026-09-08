//! Re-decode a raw archive or labelled fixture. Same inputs must hash identically.
use pumphound_domain::{
    event_id, payload_sha256, ChainEvent, LaunchpadDecoder, RawTransaction, CHAIN,
};
use pumphound_protocol_pump::PumpBondingCurveDecoder;
use pumphound_protocol_pumpswap::PumpSwapDecoder;
use serde_json::Value;
use std::path::Path;

fn load_raws(path: &Path) -> anyhow::Result<Vec<RawTransaction>> {
    let text = std::fs::read_to_string(path)?;
    if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line)?;
            let tx = v.get("transaction").cloned().unwrap_or(v);
            out.push(serde_json::from_value(tx)?);
        }
        return Ok(out);
    }
    let v: Value = serde_json::from_str(&text)?;
    if let Some(arr) = v.as_array() {
        return Ok(arr
            .iter()
            .filter_map(|row| serde_json::from_value::<RawTransaction>(row.clone()).ok())
            .collect());
    }
    Ok(vec![])
}

fn load_canonical(path: &Path) -> Vec<ChainEvent> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    v.get("events")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|ev| serde_json::from_value::<ChainEvent>(ev.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn decode_pass(raws: &[RawTransaction]) -> Vec<ChainEvent> {
    let pump = PumpBondingCurveDecoder::default();
    let swap = PumpSwapDecoder::default();
    let mut out = Vec::new();
    for tx in raws {
        if let Ok(mut ev) = pump.decode(tx) {
            out.append(&mut ev);
        }
        if let Ok(mut ev) = swap.decode(tx) {
            out.append(&mut ev);
        }
    }
    out
}

fn research_hash(raws: &[RawTransaction], events: &[ChainEvent]) -> String {
    let mut acc = String::new();
    for tx in raws {
        acc.push_str(&tx.signature);
        acc.push('|');
        acc.push_str(&tx.slot.to_string());
        acc.push('|');
        acc.push_str(&payload_sha256(
            serde_json::to_string(&tx.log_messages)
                .unwrap_or_default()
                .as_bytes(),
        ));
        acc.push('\n');
    }
    let mut evs = events.to_vec();
    evs.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    for ev in evs {
        acc.push_str(&ev.event_id);
        acc.push('|');
        acc.push_str(&ev.signature);
        acc.push('|');
        acc.push_str(&ev.slot.to_string());
        acc.push('\n');
    }
    payload_sha256(acc.as_bytes())
}

fn main() -> anyhow::Result<()> {
    let raw_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixtures/chain/sample_logs.json".into());
    let canon_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "fixtures/chain/decoded_events.json".into());
    let raws = load_raws(Path::new(&raw_path))?;
    let sidecar = load_canonical(Path::new(&canon_path));
    let decoded_a = decode_pass(&raws);
    let decoded_b = decode_pass(&raws);
    let mut events_a = decoded_a.clone();
    events_a.extend(sidecar.clone());
    let mut events_b = decoded_b;
    events_b.extend(sidecar);
    let ha = research_hash(&raws, &events_a);
    let hb = research_hash(&raws, &events_b);
    println!(
        "raws={} decoded={} sidecar_used={} hash_a={} hash_b={} match={} decoder={}",
        raws.len(),
        decoded_a.len(),
        events_a.len().saturating_sub(decoded_a.len()),
        ha,
        hb,
        ha == hb,
        pumphound_domain::DECODER_VERSION
    );
    if ha != hb {
        anyhow::bail!("archive replay hash mismatch");
    }
    let _ = event_id(CHAIN, "sig", pumphound_domain::PUMP_PROGRAM_ID, &[0], 0);
    Ok(())
}
