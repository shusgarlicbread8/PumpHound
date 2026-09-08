import { create } from "zustand";
import { api } from "../lib/api";
import type { PaperPortfolio } from "../lib/types";

type PaperState = {
  portfolios: PaperPortfolio[];
  activeId: string | null;
  error: string | null;
  load: () => Promise<void>;
  ensureDefault: () => Promise<string | null>;
  setActive: (id: string) => void;
};

export const usePaper = create<PaperState>((set, get) => ({
  portfolios: [],
  activeId: null,
  error: null,
  load: async () => {
    try {
      const res = await api.portfolios();
      const items = res.items ?? [];
      set({
        portfolios: items,
        activeId: get().activeId ?? items[0]?.portfolio_id ?? null,
        error: null,
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : "portfolios unavailable" });
    }
  },
  ensureDefault: async () => {
    await get().load();
    if (get().portfolios.length) return get().activeId ?? get().portfolios[0].portfolio_id;
    set({ error: null });
    return null;
  },
  setActive: (id) => set({ activeId: id }),
}));
