import type { GraphEdge } from "./types";
import { asArray, asNumber, asRecord, asString } from "./safe";

type Mix = { kind: string; count: number; weight: number };

/** One rendered edge per directed node pair. Relationship types stay in the mix. */
export function aggregateGraphEdges(raw: unknown): GraphEdge[] {
  const groups = new Map<
    string,
    GraphEdge & { relationship_mix: Mix[] }
  >();
  for (const item of asArray(raw)) {
    const edge = asRecord(item);
    const source = asString(edge.source, "");
    const target = asString(edge.target, "");
    const kind = asString(edge.kind, "EDGE");
    if (!source || !target) continue;
    const key = `${source}\0${target}`;
    const weight = asNumber(edge.weight) ?? 1;
    const count = asNumber(edge.count) ?? 1;
    const evidence = asArray(edge.evidence);
    const event = {
      event_id: typeof edge.event_id === "string" ? edge.event_id : null,
      signature: typeof edge.signature === "string" ? edge.signature : null,
      kind,
      weight,
      inferred: Boolean(edge.inferred),
    };
    const incomingMix = asArray(edge.relationship_mix).map((entry) => {
      const rec = asRecord(entry);
      return {
        kind: asString(rec.kind, kind),
        count: asNumber(rec.count) ?? count,
        weight: asNumber(rec.weight) ?? weight,
      };
    });
    const mixDelta: Mix[] = incomingMix.length ? incomingMix : [{ kind, count, weight }];
    const existing = groups.get(key);
    if (!existing) {
      groups.set(key, {
        source,
        target,
        kind,
        weight,
        count,
        inferred: Boolean(edge.inferred),
        relationship_mix: mixDelta,
        evidence: evidence.length ? evidence : [event],
      });
      continue;
    }
    existing.weight = (asNumber(existing.weight) ?? 0) + weight;
    existing.count = (asNumber(existing.count) ?? 0) + count;
    existing.inferred = Boolean(existing.inferred || edge.inferred);
    const bag = asArray(existing.evidence);
    bag.push(...(evidence.length ? evidence : [event]));
    existing.evidence = bag;
    for (const part of mixDelta) {
      const found = existing.relationship_mix.find((m) => m.kind === part.kind);
      if (found) {
        found.count += part.count;
        found.weight += part.weight;
      } else {
        existing.relationship_mix.push({ ...part });
      }
    }
    existing.kind = existing.relationship_mix
      .map((m) => `${m.kind}:${m.count}`)
      .join("+");
  }
  return [...groups.values()];
}
