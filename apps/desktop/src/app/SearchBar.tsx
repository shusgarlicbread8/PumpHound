import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { useUi } from "../stores/ui";

export function SearchBar() {
  const open = useUi((s) => s.searchOpen);
  const setOpen = useUi((s) => s.setSearchOpen);
  const q = useUi((s) => s.searchQuery);
  const setQ = useUi((s) => s.setSearchQuery);
  const ref = useRef<HTMLInputElement>(null);
  const nav = useNavigate();

  useEffect(() => {
    if (open) ref.current?.focus();
  }, [open]);

  if (!open) return null;
  return (
    <form
      className="absolute left-1/2 top-8 z-30 w-[420px] -translate-x-1/2"
      onSubmit={(e) => {
        e.preventDefault();
        const v = q.trim();
        if (!v) return;
        if (v.length > 40) {
          void nav({ to: "/wallets/$address", params: { address: v } });
        } else {
          void nav({ to: "/token/$launchId", params: { launchId: v } });
        }
        setOpen(false);
      }}
    >
      <input
        ref={ref}
        className="w-full rounded border border-amber bg-surface1 px-2 py-1.5"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Token / wallet / signature"
      />
    </form>
  );
}
