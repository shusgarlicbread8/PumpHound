import { create } from "zustand";
import { api } from "../lib/api";
import type { RadarRow, StreamCursor } from "../lib/types";

export type RadarSortKey =
  | "age_seconds"
  | "symbol"
  | "protocol"
  | "price_quote"
  | "curve_progress"
  | "buyers"
  | "flow_60s_quote"
  | "heuristic_rank"
  | "stance";

export type RadarFilters = {
  maxAgeMin: number | null;
  venue: "all" | "curve" | "pumpswap";
  quote: "all" | "sol" | "usdc";
  minBuyers: number | null;
  minCurve: number | null;
  excludeUnknownQuality: boolean;
  watchlistedOnly: boolean;
  heldOnly: boolean;
  modelQualifiedOnly: boolean;
};

export const defaultFilters = (): RadarFilters => ({
  maxAgeMin: null,
  venue: "all",
  quote: "all",
  minBuyers: null,
  minCurve: null,
  excludeUnknownQuality: false,
  watchlistedOnly: false,
  heldOnly: false,
  modelQualifiedOnly: false,
});

type Meaningful = {
  stance: string;
  buyers: number;
  protocol: string;
  curve: number | null;
};

function sig(row: RadarRow): Meaningful {
  return {
    stance: row.stance,
    buyers: row.buyers,
    protocol: row.protocol,
    curve: row.curve_progress,
  };
}

function changed(a: Meaningful | undefined, b: Meaningful): boolean {
  if (!a) return false;
  return a.stance !== b.stance || a.buyers !== b.buyers || a.protocol !== b.protocol || a.curve !== b.curve;
}

type RadarState = {
  ingested: Map<string, RadarRow>;
  order: string[];
  displayOrder: string[] | null;
  displayRows: Map<string, RadarRow> | null;
  paused: boolean;
  sortKey: RadarSortKey;
  sortDir: "asc" | "desc";
  sortPinned: boolean;
  newBuffer: RadarRow[];
  flashUntil: Map<string, number>;
  cursor: StreamCursor | null;
  lastUpdateAt: number | null;
  connected: boolean;
  hydrating: boolean;
  error: string | null;
  filters: RadarFilters;
  setConnected: (connected: boolean) => void;
  markUpdate: () => void;
  applySnapshot: (rows: RadarRow[], cursor?: StreamCursor | null) => void;
  setPaused: (paused: boolean) => void;
  setSort: (key: RadarSortKey) => void;
  setFilters: (patch: Partial<RadarFilters>) => void;
  flushNew: () => void;
  hydrate: () => Promise<void>;
};

function sortIds(ids: string[], rows: Map<string, RadarRow>, key: RadarSortKey, dir: "asc" | "desc"): string[] {
  const mul = dir === "asc" ? 1 : -1;
  return [...ids].sort((a, b) => {
    const ra = rows.get(a);
    const rb = rows.get(b);
    if (!ra || !rb) return 0;
    const va = ra[key];
    const vb = rb[key];
    if (va === null || va === undefined) return 1;
    if (vb === null || vb === undefined) return -1;
    if (typeof va === "number" && typeof vb === "number") return (va - vb) * mul;
    return String(va).localeCompare(String(vb)) * mul;
  });
}

export const useRadar = create<RadarState>((set, get) => ({
  ingested: new Map(),
  order: [],
  displayOrder: null,
  displayRows: null,
  paused: false,
  sortKey: "age_seconds",
  sortDir: "asc",
  sortPinned: false,
  newBuffer: [],
  flashUntil: new Map(),
  cursor: null,
  lastUpdateAt: null,
  connected: false,
  hydrating: false,
  error: null,
  filters: defaultFilters(),
  setConnected: (connected) => set({ connected }),
  markUpdate: () => set({ lastUpdateAt: Date.now() }),
  applySnapshot: (rows, cursor) => {
    const prev = get().ingested;
    const next = new Map<string, RadarRow>();
    const flash = new Map(get().flashUntil);
    const now = Date.now();
    for (const row of rows) {
      next.set(row.launch_id, row);
      if (changed(prev.get(row.launch_id) ? sig(prev.get(row.launch_id)!) : undefined, sig(row))) {
        flash.set(row.launch_id, now + 400);
      }
    }
    const order = sortIds([...next.keys()], next, get().sortKey, get().sortDir);
    const paused = get().paused;
    const pinned = get().sortPinned;
    if (paused) {
      const known = new Set(get().displayOrder ?? get().order);
      const extras = rows.filter((r) => !known.has(r.launch_id));
      set({
        ingested: next,
        order,
        newBuffer: extras.length ? [...get().newBuffer, ...extras] : get().newBuffer,
        flashUntil: flash,
        cursor: cursor ?? get().cursor,
        lastUpdateAt: now,
        error: null,
      });
      return;
    }
    if (pinned) {
      const known = new Set(get().order);
      const extras = rows.filter((r) => !known.has(r.launch_id));
      const kept = order.filter((id) => known.has(id) || !extras.some((e) => e.launch_id === id));
      set({
        ingested: next,
        order: kept.length ? kept : order,
        newBuffer: extras,
        flashUntil: flash,
        cursor: cursor ?? get().cursor,
        lastUpdateAt: now,
        error: null,
      });
      return;
    }
    set({
      ingested: next,
      order,
      newBuffer: [],
      flashUntil: flash,
      cursor: cursor ?? get().cursor,
      lastUpdateAt: now,
      error: null,
    });
  },
  setPaused: (paused) => {
    if (paused) {
      set({
        paused: true,
        displayOrder: [...get().order],
        displayRows: new Map(get().ingested),
      });
    } else {
      set({ paused: false, displayOrder: null, displayRows: null });
    }
  },
  setSort: (key) => {
    const cur = get();
    const dir = cur.sortKey === key && cur.sortDir === "asc" ? "desc" : "asc";
    const order = sortIds(cur.order, cur.ingested, key, dir);
    set({ sortKey: key, sortDir: dir, sortPinned: true, order });
  },
  setFilters: (patch) => set({ filters: { ...get().filters, ...patch } }),
  flushNew: () => {
    const { ingested, sortKey, sortDir } = get();
    const order = sortIds([...ingested.keys()], ingested, sortKey, sortDir);
    set({ order, newBuffer: [], displayOrder: get().paused ? order : null });
  },
  hydrate: async () => {
    set({ hydrating: true });
    try {
      const snap = await api.launches();
      const cursor = snap.stream_cursor
        ? {
            epoch: snap.stream_cursor.epoch,
            sequence: String(snap.stream_cursor.sequence),
          }
        : null;
      get().applySnapshot(snap.items ?? [], cursor);
      set({ hydrating: false, error: null });
    } catch (e) {
      set({
        hydrating: false,
        error: e instanceof Error ? e.message : "launch snapshot failed",
      });
    }
  },
}));

export function visibleRadarRows(): RadarRow[] {
  const s = useRadar.getState();
  const rows = s.paused && s.displayRows ? s.displayRows : s.ingested;
  const order = s.paused && s.displayOrder ? s.displayOrder : s.order;
  return order.map((id) => rows.get(id)).filter((r): r is RadarRow => Boolean(r));
}
