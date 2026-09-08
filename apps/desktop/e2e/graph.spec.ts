/**
 * Placeholder e2e: wallet/creator graph.
 * Inferred edges are not ownership.
 */
export const suite = "graph";
export const skipReason = "Playwright not installed; treat as a checklist";

export const cases = [
  "Initial render stays within ~300 nodes / 800 edges",
  "Expand, filter, collapse and time-scrub stay responsive",
  "Inspect a node: evidence IDs, as-of slot, knowledge cutoff",
  "Copy never says owned / same person for a heuristic edge",
  "Hidden-node count shown when budget truncates",
] as const;
