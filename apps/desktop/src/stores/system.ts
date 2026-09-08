import { create } from "zustand";
import type { HealthResponse, SystemStatus } from "../lib/types";

type Sample = { t: number };

type SystemState = {
  health: HealthResponse | null;
  status: SystemStatus | null;
  lastEventAt: string | null;
  msgsWindow: Sample[];
  lastMessageAt: number | null;
  qualityWarnings: string[];
  setHealth: (health: HealthResponse) => void;
  setStatus: (status: SystemStatus) => void;
  noteMessage: () => void;
  msgsPerSec: () => number;
};

export const useSystem = create<SystemState>((set, get) => ({
  health: null,
  status: null,
  lastEventAt: null,
  msgsWindow: [],
  lastMessageAt: null,
  qualityWarnings: [],
  setHealth: (health) => set({ health }),
  setStatus: (status) =>
    set({
      status,
      lastEventAt: status.last_event_at,
      qualityWarnings: [
        status.quarantined > 0 ? `${status.quarantined} quarantined decodes` : null,
        status.chain === "disconnected" ? "chain disconnected" : null,
        status.lag_ms > 2000 ? `provider lag ${status.lag_ms}ms` : null,
      ].filter((x): x is string => Boolean(x)),
    }),
  noteMessage: () => {
    const now = Date.now();
    const msgsWindow = [...get().msgsWindow, { t: now }].filter((s) => now - s.t < 5000);
    set({ msgsWindow, lastMessageAt: now });
  },
  msgsPerSec: () => {
    const now = Date.now();
    const w = get().msgsWindow.filter((s) => now - s.t < 5000);
    return w.length / 5;
  },
}));
