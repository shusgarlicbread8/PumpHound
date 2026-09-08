import { useSyncExternalStore } from "react";
import { useRadar } from "../stores/radar";
import { useSystem } from "../stores/system";

export function BottomRail() {
  const lastEvent = useSystem((s) => s.lastEventAt);
  const warnings = useSystem((s) => s.qualityWarnings);
  const status = useSystem((s) => s.status);
  const connected = useRadar((s) => s.connected);
  const now = useSyncExternalStore(
    (cb) => {
      const id = window.setInterval(cb, 500);
      return () => clearInterval(id);
    },
    () => Date.now(),
    () => 0,
  );
  const msgs = useSystem.getState().msgsPerSec();
  void now;

  return (
    <footer className="flex h-foot shrink-0 items-center gap-3 border-t border-hairline bg-surface1 px-2 text-label text-muted">
      <span className="font-mono tabular-nums">{msgs.toFixed(1)} msg/s</span>
      <span>queue {status ? String(status.decoded) : "—"}</span>
      <span className="truncate">last {lastEvent ?? "—"}</span>
      <span className={connected ? "text-positive" : "text-amber"}>{connected ? "stream" : "stale"}</span>
      <span className="ml-auto truncate text-unknown">
        {warnings.length ? warnings.join(" · ") : "no quality warnings"}
      </span>
    </footer>
  );
}
