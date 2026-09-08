import { USDC_MINT, WSOL_MINT, type Mode, type RadarRow } from "./types";
import { formatAtomic, formatQuotePerBase, quoteDecimals, quoteSymbol } from "./units";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

export function unknown(reason: string): { display: "—"; reason: string } {
  return { display: "—", reason };
}

export function formatAge(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}m ${s.toString().padStart(2, "0")}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

export function truncatePubkey(pk: string, head = 4, tail = 4): string {
  if (!pk) return "—";
  if (pk.length <= head + tail + 1) return pk;
  return `${pk.slice(0, head)}…${pk.slice(-tail)}`;
}

export function venueLabel(protocol: string): string {
  const p = protocol.toLowerCase();
  if (p === "pump_curve" || p === "pumpcurve") return "Curve";
  if (p === "pump_swap" || p === "pumpswap") return "PumpSwap";
  return protocol || "—";
}

export function isCurveVenue(protocol: string): boolean {
  const p = protocol.toLowerCase();
  return p === "pump_curve" || p === "pumpcurve";
}

export function quoteLabel(mint: string): string {
  return quoteSymbol(mint);
}

export function formatPriceQuote(price: string | null, quoteMint: string): { text: string; reason?: string } {
  if (!price) return { text: "—", reason: "no executable quote at this as-of" };
  const dec = quoteDecimals(quoteMint);
  return { text: `${formatAtomic(price, dec, 8)} ${quoteSymbol(quoteMint)}` };
}

export function formatFlow(flow: string | null, quoteMint: string): { text: string; reason?: string } {
  if (!flow) return { text: "—", reason: "60s quote flow not in this snapshot" };
  return { text: formatAtomic(flow, quoteDecimals(quoteMint), 4) };
}

export function formatCurve(progress: number | null, protocol: string): { kind: "bar" | "grad"; value: number | null; label: string } {
  if (!isCurveVenue(protocol)) {
    return { kind: "grad", value: null, label: "Graduated" };
  }
  if (progress === null || progress === undefined) {
    return { kind: "bar", value: null, label: "—" };
  }
  return { kind: "bar", value: progress, label: `${Math.round(progress * 100)}%` };
}

export function numericHeuristic(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() && value !== "HEURISTIC") {
    const n = Number(value);
    if (Number.isFinite(n)) return n;
  }
  return null;
}

export function heuristicLabel(rank: number | null): { text: string; tag: "HEURISTIC" | "NONE"; reason?: string } {
  if (rank === null || rank === undefined) {
    return { text: "—", tag: "NONE", reason: "heuristic rank not computed" };
  }
  return { text: `${Math.round(rank)}`, tag: "HEURISTIC" };
}

export function modelProbLabel(p: number | null): { text: string; tag: "MODEL" | "TARGET STATE" } {
  if (p === null || p === undefined || !Number.isFinite(p)) {
    return { text: "SEE INSPECT", tag: "TARGET STATE" };
  }
  return { text: `${(p * 100).toFixed(1)}%`, tag: "MODEL" };
}

export function normalizeMode(raw: string | undefined | null): Mode {
  const u = (raw ?? "LIVE_WATCH").toUpperCase().replace(/-/g, "_");
  if (u === "LIVE_PAPER_HEURISTIC" || u === "LIVE_PAPER") return "LIVE_PAPER_HEURISTIC";
  if (u === "LIVE_PAPER_SHADOW_ML") return "LIVE_PAPER_SHADOW_ML";
  if (u === "LIVE_PAPER_ML") return "LIVE_PAPER_ML";
  if (u === "REPLAY") return "REPLAY";
  return "LIVE_WATCH";
}

export function initials(symbol: string | null, mint: string): string {
  const s = (symbol ?? mint).replace(/[^A-Za-z0-9]/g, "");
  return (s.slice(0, 2) || "??").toUpperCase();
}

export function formatSlot(slot: string | null | undefined): string {
  if (!slot) return "—";
  return slot;
}

export function formatLag(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) {
    return "—";
  }
  if (Math.abs(ms) >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms)}ms`;
}

export function relativeAgo(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "—";
  const s = Math.max(0, (now - t) / 1000);
  return `${s.toFixed(1)}s`;
}

export function staleCaption(lastUpdateMs: number | null, now = Date.now()): string {
  if (lastUpdateMs === null) return "STALE · last update —";
  const s = Math.max(0, (now - lastUpdateMs) / 1000);
  return `STALE · last update ${s.toFixed(1)} s`;
}

export function qualityReason(q: RadarRow["quality"] | undefined): string {
  if (!q) return "quality not attached";
  if (q.reasons?.length) return q.reasons.join("; ");
  if (q.missing_fields?.length) return `missing: ${q.missing_fields.join(", ")}`;
  if (q.stale_components?.length) return `stale: ${q.stale_components.join(", ")}`;
  return `grade ${q.grade}, coverage ${q.coverage}`;
}

export function attributeString(attrs: Record<string, unknown> | undefined, key: string): string | null {
  const v = attrs?.[key];
  return typeof v === "string" && v.length ? v : null;
}

export function solscanAccount(pk: string): string {
  return `https://solscan.io/account/${encodeURIComponent(pk)}`;
}

export function solscanTx(sig: string): string {
  return `https://solscan.io/tx/${encodeURIComponent(sig)}`;
}

export function isQuoteMint(mint: string): boolean {
  return mint === WSOL_MINT || mint === USDC_MINT;
}

export function estimatedMc(row: RadarRow): { text: string; reason?: string } {
  if (!row.price_quote) {
    return { text: "—", reason: "estimated market cap needs executable price × supply" };
  }
  return {
    text: "—",
    reason: "token supply is not on the radar row; open the token workspace",
  };
}

export function curveReservePrice(
  virtualQuote: string | null,
  virtualBase: string | null,
  quoteMint: string,
): { text: string; reason?: string } {
  const formatted = formatQuotePerBase(
    virtualQuote,
    virtualBase,
    quoteDecimals(quoteMint),
    6,
    8,
  );
  if (!formatted) {
    return { text: "—", reason: "reserves missing or zero; not an executable fill" };
  }
  return { text: `${formatted} ${quoteSymbol(quoteMint)} / token` };
}

export const ageLabel = formatAge;
export const shortKey = truncatePubkey;
export const quoteAsset = quoteLabel;
export function dash(v: string | number | null | undefined): string {
  if (v === null || v === undefined || v === "") return "—";
  return String(v);
}
export function pct(v: number | null | undefined): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  return `${(v * 100).toFixed(1)}%`;
}

export function newId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) return crypto.randomUUID();
  return `00000000-0000-4000-8000-${Math.random().toString(16).slice(2, 14).padEnd(12, "0")}`;
}
