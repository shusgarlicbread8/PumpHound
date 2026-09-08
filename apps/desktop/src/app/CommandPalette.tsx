import * as Dialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { api } from "../lib/api";
import { useUi } from "../stores/ui";

const JUMPS = [
  { label: "Home", to: "/" },
  { label: "Radar", to: "/radar" },
  { label: "Trade", to: "/trade" },
  { label: "Paper", to: "/paper" },
  { label: "Strategies", to: "/strategies" },
  { label: "Models", to: "/models" },
  { label: "Research", to: "/research" },
  { label: "System", to: "/system" },
  { label: "Graph", to: "/graph" },
  { label: "Replay", to: "/replay" },
  { label: "Settings", to: "/settings" },
];

export function CommandPalette() {
  const open = useUi((s) => s.commandOpen);
  const setOpen = useUi((s) => s.setCommandOpen);
  const q = useUi((s) => s.searchQuery);
  const setQ = useUi((s) => s.setSearchQuery);
  const nav = useNavigate();
  const [local, setLocal] = useState("");
  const search = useQuery({
    queryKey: ["search", q],
    queryFn: () => api.search(q),
    enabled: open && q.length >= 8,
  });

  const go = (to: string) => {
    setOpen(false);
    void nav({ to });
  };

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content className="fixed left-1/2 top-24 z-50 w-[520px] -translate-x-1/2 rounded border border-hairline bg-surface1 p-2 shadow-trail">
          <Dialog.Title className="sr-only">Command palette</Dialog.Title>
          <input
            autoFocus
            className="w-full rounded border border-hairline bg-canvas px-2 py-1.5"
            placeholder="Jump or search mint / wallet / signature"
            value={local}
            onChange={(e) => {
              setLocal(e.target.value);
              setQ(e.target.value);
            }}
          />
          <ul className="mt-2 max-h-72 overflow-auto">
            {JUMPS.filter((j) => j.label.toLowerCase().includes(local.toLowerCase()) || !local).map((j) => (
              <li key={j.to}>
                <button
                  type="button"
                  className="w-full px-2 py-1 text-left text-body hover:bg-surface2"
                  onClick={() => go(j.to)}
                >
                  {j.label}
                </button>
              </li>
            ))}
            {search.data?.items.map((hit) => (
              <li key={hit.launch_id}>
                <button
                  type="button"
                  className="w-full px-2 py-1 text-left font-mono text-label hover:bg-surface2"
                  onClick={() => {
                    setOpen(false);
                    void nav({ to: "/token/$launchId", params: { launchId: hit.launch_id } });
                  }}
                >
                  {hit.mint} · {hit.creator.slice(0, 8)}
                </button>
              </li>
            ))}
          </ul>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
