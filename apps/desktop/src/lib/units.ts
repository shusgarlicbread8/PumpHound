import { USDC_MINT, WSOL_MINT } from "./types";

const ATOMIC_RE = /^-?\d+$/;
const DECIMAL_RE = /^\d+(\.\d+)?$/;

/** Keep money as decimal strings. Never use Number() for atomic arithmetic. */
export function isAtomicString(value: string): boolean {
  return ATOMIC_RE.test(value);
}

export function decimalishToString(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value === "string") {
    const t = value.trim();
    return t.length ? t : null;
  }
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "number") {
    if (!Number.isFinite(value)) return null;
    return Math.trunc(value).toString();
  }
  return null;
}

export function requireAtomic(value: unknown, reason: string): string {
  const s = decimalishToString(value);
  if (!s || !isAtomicString(s)) {
    throw new Error(reason);
  }
  return s;
}

export function quoteDecimals(mint: string): number {
  if (mint === WSOL_MINT) return 9;
  if (mint === USDC_MINT) return 6;
  return 6;
}

export function quoteSymbol(mint: string): string {
  if (mint === WSOL_MINT) return "SOL";
  if (mint === USDC_MINT) return "USDC";
  return "QUOTE";
}

export function formatAtomic(atomic: string, decimals: number, maxFrac = 6): string {
  if (!isAtomicString(atomic)) return "—";
  const neg = atomic.startsWith("-");
  const digits = (neg ? atomic.slice(1) : atomic).replace(/^0+(?=\d)/, "");
  const pad = digits.padStart(decimals + 1, "0");
  const split = pad.length - decimals;
  const whole = pad.slice(0, split) || "0";
  let frac = pad.slice(split);
  frac = frac.replace(/0+$/, "");
  if (maxFrac >= 0) frac = frac.slice(0, maxFrac);
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const body = frac ? `${grouped}.${frac}` : grouped;
  return neg ? `−${body}` : body;
}

export function parseDecimalToAtomic(input: string, decimals: number): string {
  const trimmed = input.trim();
  if (!DECIMAL_RE.test(trimmed)) {
    throw new Error("invalid decimal");
  }
  const [wholeRaw, fracRaw = ""] = trimmed.split(".");
  if (fracRaw.length > decimals) {
    throw new Error("too many fractional digits");
  }
  const frac = fracRaw.padEnd(decimals, "0");
  const combined = `${wholeRaw}${frac}`.replace(/^0+/, "") || "0";
  if (!isAtomicString(combined)) throw new Error("invalid atomic");
  return combined;
}

export function compareAtomic(a: string, b: string): number {
  const an = a.startsWith("-");
  const bn = b.startsWith("-");
  if (an !== bn) return an ? -1 : 1;
  const ad = an ? a.slice(1) : a;
  const bd = bn ? b.slice(1) : b;
  const aa = ad.replace(/^0+/, "") || "0";
  const bb = bd.replace(/^0+/, "") || "0";
  if (aa.length !== bb.length) {
    const longer = aa.length > bb.length ? 1 : -1;
    return an ? -longer : longer;
  }
  if (aa === bb) return 0;
  const cmp = aa > bb ? 1 : -1;
  return an ? -cmp : cmp;
}

export function addAtomic(a: string, b: string): string {
  return (BigInt(a) + BigInt(b)).toString();
}

export function subAtomic(a: string, b: string): string {
  return (BigInt(a) - BigInt(b)).toString();
}

/** Display-only ratio. Returns null instead of a JS Number price when inputs are unusable. */
export function formatQuotePerBase(
  quoteAtomic: string | null,
  baseAtomic: string | null,
  quoteDec: number,
  baseDec: number,
  frac = 8,
): string | null {
  if (!quoteAtomic || !baseAtomic || !isAtomicString(quoteAtomic) || !isAtomicString(baseAtomic)) {
    return null;
  }
  if (baseAtomic === "0") return null;
  try {
    const scale = 10n ** BigInt(Math.max(frac + baseDec - quoteDec, 0));
    const q = BigInt(quoteAtomic) * scale;
    const d = BigInt(baseAtomic);
    const whole = q / d;
    const asStr = whole.toString().padStart(frac + 1, "0");
    const split = asStr.length - frac;
    const ip = asStr.slice(0, split);
    const fp = asStr.slice(split).replace(/0+$/, "");
    return fp ? `${ip}.${fp}` : ip;
  } catch {
    return null;
  }
}
