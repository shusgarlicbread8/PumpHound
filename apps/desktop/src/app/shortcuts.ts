import { normalizeMode } from "../lib/format";
import { useRadar } from "../stores/radar";
import { useSystem } from "../stores/system";
import { useUi } from "../stores/ui";

function typingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el.isContentEditable;
}

type Go = (to: "/" | "/radar" | "/trade" | "/paper" | "/models" | "/system") => void;

export function bindShortcuts(go: Go): () => void {
  let pendingG = false;
  let gTimer: ReturnType<typeof setTimeout> | null = null;

  const onKey = (e: KeyboardEvent) => {
    if (typingTarget(e.target)) return;
    const meta = e.metaKey || e.ctrlKey;
    if (meta && e.key.toLowerCase() === "k") {
      e.preventDefault();
      useUi.getState().setCommandOpen(true);
      return;
    }
    if (e.key === "/" && !meta) {
      e.preventDefault();
      useUi.getState().setSearchOpen(true);
      return;
    }
    if (e.key === "Escape") {
      useUi.getState().closeDrawer();
      useUi.getState().setCommandOpen(false);
      useUi.getState().setSearchOpen(false);
      useUi.getState().closePaperTicket();
      pendingG = false;
      return;
    }
    if (e.key === " " || e.code === "Space") {
      const mode = normalizeMode(useSystem.getState().health?.mode ?? useSystem.getState().status?.mode);
      if (mode === "REPLAY") {
        e.preventDefault();
        useRadar.getState().setPaused(!useRadar.getState().paused);
      }
      return;
    }
    if (e.key.toLowerCase() === "b") {
      useUi.getState().openPaperTicket("buy");
      go("/paper");
      return;
    }
    if (e.key.toLowerCase() === "s" && !pendingG) {
      useUi.getState().openPaperTicket("sell");
      go("/paper");
      return;
    }
    if (e.key.toLowerCase() === "g") {
      pendingG = true;
      if (gTimer) clearTimeout(gTimer);
      gTimer = setTimeout(() => {
        pendingG = false;
      }, 700);
      return;
    }
    if (pendingG) {
      const k = e.key.toLowerCase();
      pendingG = false;
      if (k === "h") go("/");
      if (k === "r") go("/radar");
      if (k === "t") go("/trade");
      if (k === "p") go("/paper");
      if (k === "m") go("/models");
      if (k === "s") go("/system");
    }
  };

  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}
