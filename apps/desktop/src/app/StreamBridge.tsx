import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { api } from "../lib/api";
import { startStream } from "../lib/stream";
import type { StreamMessage } from "../lib/types";
import { usePaper } from "../stores/paper";
import { useRadar } from "../stores/radar";
import { useSystem } from "../stores/system";

export function StreamBridge() {
  const healthQ = useQuery({ queryKey: ["health"], queryFn: api.health, refetchInterval: 4000 });
  const statusQ = useQuery({ queryKey: ["system"], queryFn: api.systemStatus, refetchInterval: 4000 });
  const hydrate = useRadar((s) => s.hydrate);
  const apply = useRadar((s) => s.applySnapshot);
  const setConnected = useRadar((s) => s.setConnected);
  const mark = useRadar((s) => s.markUpdate);
  const setHealth = useSystem((s) => s.setHealth);
  const setStatus = useSystem((s) => s.setStatus);
  const note = useSystem((s) => s.noteMessage);
  const loadPaper = usePaper((s) => s.load);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (healthQ.data) setHealth(healthQ.data);
  }, [healthQ.data, setHealth]);
  useEffect(() => {
    if (statusQ.data) setStatus(statusQ.data);
  }, [statusQ.data, setStatus]);

  useEffect(() => {
    void hydrate();
    void loadPaper();
    const scheduleHydrate = () => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        void hydrate();
      }, 180);
    };
    const stop = startStream({
      onConnectionChange: (c) => {
        setConnected(c);
        if (c) void hydrate();
      },
      onMessage: () => {
        note();
        mark();
      },
      onSnapshot: () => {
        scheduleHydrate();
      },
      onDelta: (msg: StreamMessage) => {
        const payload = msg.payload;
        if (payload && typeof payload === "object" && "launch_id" in payload && "mint" in payload) {
          const row = payload as import("../lib/types").RadarRow;
          const cur = useRadar.getState();
          const next = new Map(cur.ingested);
          next.set(row.launch_id, row);
          apply([...next.values()], { epoch: msg.epoch, sequence: msg.sequence });
        } else {
          scheduleHydrate();
        }
      },
      onHeartbeat: (msg) => {
        const p = msg.payload;
        if (p && typeof p === "object") {
          const cur = useSystem.getState().status;
          if (cur && "lag_ms" in (p as object)) {
            const hp = p as { chain?: string; lag_ms?: number };
            setStatus({
              ...cur,
              chain: hp.chain ?? cur.chain,
              lag_ms: hp.lag_ms ?? cur.lag_ms,
              last_slot: msg.as_of_slot ?? cur.last_slot,
            });
          }
        }
      },
      onEvent: () => scheduleHydrate(),
      onResyncRequired: () => {
        void hydrate();
      },
      onError: () => {
        /* keep last known rows */
      },
    });
    return () => {
      stop();
      if (timer.current) clearTimeout(timer.current);
    };
  }, [apply, hydrate, loadPaper, mark, note, setConnected, setStatus]);

  return null;
}
