/**
 * Placeholder e2e: keyboard, contrast-independent risk, viewports.
 */
export const suite = "accessibility";
export const skipReason = "Playwright not installed; treat as a checklist";

export const cases = [
  "Tab order reaches search, radar, token, paper, settings",
  "Risk flags are not colour-only (icon/text + colour)",
  "1280×800 and 1920×1080 layouts do not clip primary rails",
  "prefers-reduced-motion disables non-essential animation",
  "Command palette and shortcuts match documented keys",
] as const;
