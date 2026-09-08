import * as Dialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { useUi } from "../../stores/ui";

export function LedgerDrawer() {
  const open = useUi((s) => s.drawer === "ledger");
  const id = useUi((s) => s.drawerSubject);
  const close = useUi((s) => s.closeDrawer);
  const q = useQuery({
    queryKey: ["ledger", id],
    queryFn: () => api.ledger(id!),
    enabled: open && Boolean(id),
  });
  return (
    <Dialog.Root open={open} onOpenChange={(v) => !v && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content className="fixed right-0 top-0 z-50 h-full w-[420px] border-l border-hairline bg-surface1 p-3">
          <Dialog.Title className="text-header">Paper ledger</Dialog.Title>
          {q.data ? (
            <div className="mt-2 text-body">
              <div>Portfolio {q.data.portfolio_id}</div>
              <div>Reconciled {q.data.reconciled ? "yes" : "no"}</div>
              {q.data.items.length ? (
                <ul className="mt-2 font-mono text-label">
                  {q.data.items.map((item, i) => (
                    <li key={i}>{JSON.stringify(item)}</li>
                  ))}
                </ul>
              ) : (
                <EmptyState title="No movements" reason="Ledger items are written on fill." />
              )}
            </div>
          ) : (
            <EmptyState title="Ledger" reason={q.error?.message ?? "Select a portfolio."} />
          )}
          <button type="button" className="mt-4 rounded border border-hairline px-2 py-1" onClick={close}>
            Close
          </button>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
