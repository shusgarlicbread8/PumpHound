import { decimalishToString, formatAtomic, quoteDecimals, quoteSymbol } from "./units";
import { WSOL_MINT } from "./types";

export type MoneyState = "KNOWN" | "UNKNOWN" | "NOT_APPLICABLE" | "PENDING";

export type DisplayMoney = {
  state: MoneyState;
  display: string;
  symbol: string | null;
  usd: string;
  atomic: string | null;
  decimals: number | null;
};

/** Format quote-native atomic amounts. Never show a bare integer as P&L. */
export function formatQuoteMoney(
  atomic: unknown,
  mint: string = WSOL_MINT,
  maxFrac = 4,
): DisplayMoney {
  const raw = decimalishToString(atomic);
  if (!raw) {
    return {
      state: "UNKNOWN",
      display: "UNKNOWN",
      symbol: quoteSymbol(mint),
      usd: "USD unavailable",
      atomic: null,
      decimals: quoteDecimals(mint),
    };
  }
  const decimals = quoteDecimals(mint);
  const symbol = quoteSymbol(mint);
  const formatted = formatAtomic(raw, decimals, maxFrac);
  if (formatted === "—") {
    return {
      state: "UNKNOWN",
      display: "UNKNOWN",
      symbol,
      usd: "USD unavailable",
      atomic: raw,
      decimals,
    };
  }
  return {
    state: "KNOWN",
    display: `${formatted} ${symbol}`,
    symbol,
    usd: "USD unavailable",
    atomic: raw,
    decimals,
  };
}

export function formatQuoteMoneyFromReport(value: unknown, mint: string = WSOL_MINT): string {
  if (value && typeof value === "object") {
    const rec = value as Record<string, unknown>;
    if (typeof rec.display === "string" && rec.display.trim()) return rec.display;
    if (rec.atomic != null) return formatQuoteMoney(rec.atomic, String(rec.asset_mint ?? mint)).display;
  }
  return formatQuoteMoney(value, mint).display;
}
