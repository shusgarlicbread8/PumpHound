/**
 * Placeholder e2e: live terminal (Radar → token → evidence).
 * Enable with Playwright against Vite `http://localhost:1420` once capture is visible.
 */
export const suite = "live-terminal";
export const skipReason = "Playwright not installed; treat as a checklist";

export const cases = [
  "Radar shows source label LIVE MAINNET / RPC FALLBACK or LASERSTREAM or REPLAY",
  "New launch row appears without a full page refresh",
  "Saved filter remains stable when rows stream in",
  "Open a token: header, tape, curve, price chart, quality badge",
  "Evidence drawer shows signature/slot and decoder version",
  "Disconnect/reconnect stream: health transitions, tape not corrupted",
  "Fixture sessions keep a persistent REPLAY banner",
] as const;
