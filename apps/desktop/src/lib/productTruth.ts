import { asArray, asRecord, asString } from "./safe";

export type TruthFilters = { book_id?: string; experiment_id?: string; strategy?: string; state?: string; window?: string; tab?: string };
export function truthFilters(search: Record<string, unknown>): TruthFilters {
  const result: TruthFilters = {};
  for (const key of ["book_id", "experiment_id", "strategy", "state", "window", "tab"] as const) {
    if (typeof search[key] === "string" && search[key]) result[key] = search[key];
  }
  result.window = result.window === "today" ? "today" : "all";
  if (result.state) result.state = result.state.toUpperCase();
  return result;
}
export function tradeHref(filters: TruthFilters = {}): string {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) if (value) query.set(key, value);
  return `/trade${query.size ? `?${query}` : ""}`;
}
export function traceHref(id: unknown, replay = false): string {
  return `/replay?trace=${encodeURIComponent(asString(id, ""))}${replay ? "&mode=replay" : ""}`;
}
export function normalizeTruth(value: unknown) {
  const row = asRecord(value);
  return { ...row, scope: asRecord(row.scope), summary: asRecord(row.summary),
    limits: asRecord(row.limits),
    books: asArray(row.books).map(asRecord),
    available_books: asArray(row.available_books ?? row.books).map(asRecord),
    positions: asArray(row.positions).map(asRecord),
    orders: asArray(row.orders).map(asRecord), activity: asArray(row.activity).map(asRecord) };
}
export function eventTime(event: Record<string, unknown>): string {
  return asString(event.known_at ?? event.at ?? event.timestamp ?? event.occurred_at, "");
}
/** Fail closed: undated events cannot enter a historical knowledge view. */
export function eventsAt(events: unknown, at: string): Record<string, unknown>[] {
  const cutoff = Date.parse(at);
  if (!Number.isFinite(cutoff)) return [];
  return asArray(events).map(asRecord).filter(event => {
    const known = Date.parse(eventTime(event));
    const occurred = Date.parse(asString(event.at ?? event.timestamp ?? event.occurred_at, ""));
    return Number.isFinite(known) && Number.isFinite(occurred) && known <= cutoff && occurred <= cutoff;
  }).sort((a, b) => Date.parse(eventTime(a)) - Date.parse(eventTime(b)));
}
export function activityGroup(kind: unknown): string {
  const value = asString(kind, "").toUpperCase();
  if (/RISK/.test(value)) return "Risk";
  if (/LABEL|MODEL|TRAIN|PROMOT/.test(value)) return "Learning";
  if (/DECISION/.test(value)) return "Decisions";
  if (/FILL/.test(value)) return "Fills";
  if (/POSITION/.test(value)) return "Positions";
  if (/ORDER/.test(value)) return "Orders";
  return "Other";
}
