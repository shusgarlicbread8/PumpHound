import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { useEffect } from "react";
import { EvidenceDrawer } from "../components/EvidenceDrawer";
import { PageErrorBoundary } from "../components/PageErrorBoundary";
import { LedgerDrawer } from "../features/paper/LedgerDrawer";
import { ScoreDetailsDrawer } from "../features/token/ScoreDetailsDrawer";
import { BottomRail } from "./BottomRail";
import { CommandPalette } from "./CommandPalette";
import { LeftNav } from "./LeftNav";
import { SearchBar } from "./SearchBar";
import { bindShortcuts } from "./shortcuts";
import { TopRail } from "./TopRail";

export function Shell() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  useEffect(
    () =>
      bindShortcuts((to) => {
        void navigate({ to });
      }),
    [navigate],
  );

  return (
    <div className="flex h-full flex-col bg-canvas text-ink">
      <TopRail />
      <div className="relative flex min-h-0 flex-1">
        <LeftNav />
        <main className="relative min-w-0 flex-1">
          <SearchBar />
          <PageErrorBoundary resetKey={pathname}>
            <Outlet />
          </PageErrorBoundary>
        </main>
      </div>
      <BottomRail />
      <CommandPalette />
      <EvidenceDrawer />
      <ScoreDetailsDrawer />
      <LedgerDrawer />
    </div>
  );
}
