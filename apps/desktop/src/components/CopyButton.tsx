import { useState } from "react";

export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const [ok, setOk] = useState(false);
  return (
    <button
      type="button"
      className="rounded border border-hairline px-1.5 py-0.5 text-label text-muted hover:bg-surface2 hover:text-ink"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setOk(true);
          window.setTimeout(() => setOk(false), 900);
        } catch {
          setOk(false);
        }
      }}
    >
      {ok ? "Copied" : label}
    </button>
  );
}
