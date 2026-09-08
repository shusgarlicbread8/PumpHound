"""CLI: python -m pumphound_ml.graph.refresh <manifest>"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from pumphound_ml.clock import parse_cutoff
from pumphound_ml.extraction.history import extract_wallet_history
from pumphound_ml.extraction.transfers import extract_transfers
from pumphound_ml.graph.cooccurrence import build_early_observations, score_pairs
from pumphound_ml.graph.communities import funding_families, leiden_communities
from pumphound_ml.graph.facts import build_facts
from pumphound_ml.graph.funding import FundingBudget, trace_funding
from pumphound_ml.graph.intent import infer_intent
from pumphound_ml.graph.lineage import assign_families
from pumphound_ml.graph.reputation import creator_reputation, wallet_reputation
from pumphound_ml.io import dump_json, load_jsonl, load_yaml, resolve_path, sha256_canonical
from pumphound_ml.types import load_events, to_plain


def refresh_graph(manifest_path: str) -> dict:
    manifest = load_yaml(manifest_path) if str(manifest_path).endswith((".yaml", ".yml")) else json.loads(
        Path(resolve_path(manifest_path)).read_text(encoding="utf-8")
    )
    events_uri = manifest.get("events_uri") or manifest.get("events_path") or "fixtures/replay/events.jsonl"
    graph_cfg = load_yaml(manifest.get("graph_config", "ml/configs/graph-v1.yaml"))
    cutoff = parse_cutoff(manifest.get("knowledge_cutoff") or "2026-09-07T12:00:00.000000Z")
    cutoff_slot = int(manifest.get("cutoff_slot") or 0)
    events = load_events(load_jsonl(events_uri))
    batch = build_facts(events, cutoff, cutoff_slot or None)
    transfers = extract_transfers(events, cutoff, batch.cutoff_slot)
    history = extract_wallet_history(events, cutoff, batch.cutoff_slot)

    budget = FundingBudget.from_config(graph_cfg.get("funding"))
    wallets = [n.public_key for n in batch.nodes if n.node_type == "Wallet" and n.public_key]
    funding = [trace_funding(w, batch.cutoff_slot, budget, facts=batch) for w in wallets[:64]]
    observations = build_early_observations(batch)
    pairs = score_pairs(
        observations,
        min_shared=int(graph_cfg.get("cooccurrence", {}).get("durable_min_shared_launches", 3)),
        min_score=float(graph_cfg.get("cooccurrence", {}).get("durable_min_score", 0.65)),
        weights=graph_cfg.get("cooccurrence", {}).get("weights"),
    )
    families = funding_families(funding, as_of_slot=batch.cutoff_slot)
    communities = leiden_communities(pairs, as_of_slot=batch.cutoff_slot)
    communities, lineage = assign_families([], communities)

    creators = {
        n.attributes.get("creator")
        for n in batch.nodes
        if n.node_type == "Launch" and n.attributes.get("creator")
    }
    reps = [
        creator_reputation(
            creator=str(c),
            as_of_slot=batch.cutoff_slot,
            knowledge_cutoff=cutoff,
            graduated=0,
            matured=0,
            collapses=0,
            prior_alpha=float(graph_cfg.get("reputation", {}).get("prior_alpha", 1.0)),
            prior_beta=float(graph_cfg.get("reputation", {}).get("prior_beta", 3.0)),
        )
        for c in creators
        if c
    ]
    intents = []
    for w in wallets[:16]:
        intents.extend(
            infer_intent(
                subject_type="wallet",
                subject_id=w,
                as_of_slot=batch.cutoff_slot,
                knowledge_cutoff=cutoff,
                pairs=pairs,
                funding=next((f for f in funding if f.wallet == w), None),
                history_complete=history.get(w, {}).get("complete", False) if isinstance(history, dict) else False,
            )
        )

    artifact = {
        "algorithm_version": graph_cfg.get("algorithm_version", "graph-v1"),
        "knowledge_cutoff": cutoff.isoformat(),
        "cutoff_slot": batch.cutoff_slot,
        "fact_count": len(batch.facts),
        "node_count": len(batch.nodes),
        "rejected_future_count": batch.rejected_future_count,
        "durable_pair_count": sum(1 for p in pairs if p.durable),
        "cluster_count": len(communities) + len(families),
        "transfer_count": len(transfers),
        "content_sha256": sha256_canonical(
            {
                "facts": [f.fact_id for f in batch.facts],
                "pairs": [(p.wallet_a, p.wallet_b, p.score, p.durable) for p in pairs],
            }
        ),
        "lineage_events": [to_plain(e) for e in lineage.events],
        "intent": [h.to_record() for h in intents],
        "reputation": [to_plain(r) for r in reps],
    }
    out_dir = Path(manifest.get("output_uri") or "artifacts/graph")
    dump_json(out_dir / "graph-refresh.json", artifact)
    return artifact


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Refresh PumpHound graph projections")
    parser.add_argument("manifest", help="graph refresh manifest (yaml/json)")
    args = parser.parse_args(argv)
    artifact = refresh_graph(args.manifest)
    print(json.dumps(artifact, indent=2, default=str))
    return 0


if __name__ == "__main__":
    sys.exit(main())
