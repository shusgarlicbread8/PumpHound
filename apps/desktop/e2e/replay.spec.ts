/**
 * Placeholder e2e: replay controls over fixtures/replay/manifest.json.
 */
export const suite = "replay";
export const skipReason = "Playwright not installed; treat as a checklist";

export const cases = [
  "Loading the synthetic manifest shows REPLAY and SYNTHETIC — not live mainnet",
  "Pause, single-event step and speed controls move the virtual clock",
  "Radar/token/paper screens follow the replay clock",
  "Two same-seed runs match content hash, ignoring wall-clock computed_at",
  "Corrupt manifest is rejected (see docs/runbooks/replay-manifest.md)",
] as const;
