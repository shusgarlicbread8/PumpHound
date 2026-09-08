/**
 * Placeholder e2e: paper desk. LIVE_PAPER or REPLAY only.
 */
export const suite = "paper";
export const skipReason = "Playwright not installed; treat as a checklist";

export const cases = [
  "Watch mode rejects order ticket with UNSUPPORTED_EXECUTION_MODE / disabled submit",
  "Create portfolio; cash shown as atomic strings not JS floats",
  "Quote then buy: order QUEUED → arrival → fill → position",
  "Sell reduces lots without negative spendable cash",
  "Slippage / stale quote / no snapshot surfaces a named error",
  "Breaker freeze blocks entries; exits remain available",
  "No seed phrase, keypair or submit control exists in the UI",
] as const;
