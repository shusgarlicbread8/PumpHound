import * as Dialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import { useUi } from "../../stores/ui";
import { scoreDefinition, scoreTarget } from "./MarketMetrics";

export function ScoreDetailsDrawer() {
  const open = useUi((s) => s.drawer === "score");
  const id = useUi((s) => s.drawerSubject);
  const launchId = useUi((s) => s.selectedLaunchId);
  const close = useUi((s) => s.closeDrawer);
  const def = scoreDefinition(id);
  const target = scoreTarget(id);
  const q = useQuery({
    queryKey: ["model-states", launchId],
    queryFn: () => api.modelStates(launchId!),
    enabled: open && Boolean(launchId && target),
  });
  const state = asArray(q.data?.targets).map(asRecord).find((item) => item.target === target);
  const score = asNumber(state?.score);
  return (
    <Dialog.Root open={open} onOpenChange={(v) => !v && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content className="fixed right-0 top-0 z-50 h-full w-[420px] border-l border-hairline bg-surface1 p-3">
          <Dialog.Title className="text-header">{def.title}</Dialog.Title>
          <p className="mt-2 text-body text-muted">{def.body}</p>
          <dl className="mt-3 space-y-2 text-body">
            <div>
              <div className="label">Current value</div>
              <div className={score == null ? "text-unknown" : "font-mono"}>
                {target
                  ? score == null
                    ? asString(state?.state, q.isError ? "MODEL ERROR" : "PENDING").replaceAll("_", " ")
                    : score.toFixed(4)
                  : "NOT APPLICABLE"}
              </div>
              {target ? <div className="text-label text-muted">{asString(state?.reason, q.error?.message ?? "Waiting for target evidence")}</div> : null}
            </div>
            <div>
              <div className="label">Calibration band</div>
              <div className="text-unknown">UNKNOWN · no verified live calibration aggregate</div>
            </div>
            <div>
              <div className="label">Feature versions</div>
              <div className="text-unknown">{asString(state?.feature_snapshot_id, "UNKNOWN")}</div>
            </div>
            <div>
              <div className="label">Heuristic</div>
              <div className="text-amber">HEURISTIC · labelled separately from probability</div>
            </div>
          </dl>
          <button type="button" className="mt-4 rounded border border-hairline px-2 py-1" onClick={close}>
            Close
          </button>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
