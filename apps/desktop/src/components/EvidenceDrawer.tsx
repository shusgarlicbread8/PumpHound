import * as Dialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import { solscanTx } from "../lib/format";
import { useUi } from "../stores/ui";
import { EmptyState } from "./EmptyState";
import { QualityBadge } from "./QualityBadge";

export function EvidenceDrawer() {
  const open = useUi((s) => s.drawer === "evidence");
  const id = useUi((s) => s.drawerSubject);
  const close = useUi((s) => s.closeDrawer);
  const q = useQuery({
    queryKey: ["evidence", id],
    queryFn: () => api.evidence(id!),
    enabled: open && Boolean(id),
  });

  return (
    <Dialog.Root open={open} onOpenChange={(v) => !v && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content className="fixed right-0 top-0 z-50 flex h-full w-[420px] flex-col border-l border-hairline bg-surface1 p-3 shadow-trail">
          <Dialog.Title className="text-header">Evidence</Dialog.Title>
          <Dialog.Description className="mb-3 text-label text-muted">
            Source-grounded record. No inferred ownership.
          </Dialog.Description>
          {q.isLoading ? (
            <div className="text-muted">Loading evidence…</div>
          ) : q.isError ? (
            <EmptyState title="Evidence unavailable" reason={q.error.message} />
          ) : q.data ? (
            <dl className="space-y-2 text-body">
              <Row k="ID" v={q.data.evidence_id} />
              <Row k="Class" v={q.data.class ?? "—"} />
              <Row k="Note" v={typeof q.data.note === "string" ? q.data.note : "—"} />
              <Row k="Signature" v={typeof q.data.signature === "string" ? q.data.signature : "—"} />
              <Row k="Slot" v={typeof q.data.slot === "string" ? q.data.slot : "—"} />
              <Row k="Source" v={typeof q.data.source === "string" ? q.data.source : "—"} />
              <Row
                k="Decoder"
                v={typeof q.data.decoder_version === "string" ? q.data.decoder_version : "—"}
              />
              {typeof q.data.signature === "string" && q.data.signature.length > 20 ? (
                <a
                  className="text-info underline"
                  href={solscanTx(q.data.signature)}
                  target="_blank"
                  rel="noreferrer"
                >
                  Open signature
                </a>
              ) : null}
              <QualityBadge quality={null} />
            </dl>
          ) : (
            <EmptyState title="No evidence id" reason="Select a flag or tape row first." />
          )}
          <button
            type="button"
            className="mt-auto rounded border border-hairline px-2 py-1 text-body hover:bg-surface2"
            onClick={close}
          >
            Close
          </button>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div>
      <div className="label">{k}</div>
      <div className="break-all font-mono text-body">{v}</div>
    </div>
  );
}
