import { create } from "zustand";

export type DrawerKind = "evidence" | "score" | "ledger" | "knowable" | null;

type UiState = {
  navExpanded: boolean;
  selectedLaunchId: string | null;
  selectedMint: string | null;
  commandOpen: boolean;
  searchOpen: boolean;
  searchQuery: string;
  drawer: DrawerKind;
  drawerSubject: string | null;
  paperSide: "buy" | "sell";
  paperTicketOpen: boolean;
  lastWallet: string | null;
  lastCreator: string | null;
  toggleNav: () => void;
  selectLaunch: (launchId: string, mint: string) => void;
  setCommandOpen: (open: boolean) => void;
  setSearchOpen: (open: boolean) => void;
  setSearchQuery: (q: string) => void;
  openDrawer: (kind: Exclude<DrawerKind, null>, subject: string) => void;
  closeDrawer: () => void;
  openPaperTicket: (side: "buy" | "sell") => void;
  closePaperTicket: () => void;
  rememberWallet: (address: string) => void;
  rememberCreator: (address: string) => void;
};

export const useUi = create<UiState>((set) => ({
  navExpanded: false,
  selectedLaunchId: null,
  selectedMint: null,
  commandOpen: false,
  searchOpen: false,
  searchQuery: "",
  drawer: null,
  drawerSubject: null,
  paperSide: "buy",
  paperTicketOpen: false,
  lastWallet: null,
  lastCreator: null,
  toggleNav: () => set((s) => ({ navExpanded: !s.navExpanded })),
  selectLaunch: (launchId, mint) => set({ selectedLaunchId: launchId, selectedMint: mint }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  setSearchOpen: (searchOpen) => set({ searchOpen }),
  setSearchQuery: (searchQuery) => set({ searchQuery }),
  openDrawer: (drawer, drawerSubject) => set({ drawer, drawerSubject }),
  closeDrawer: () => set({ drawer: null, drawerSubject: null }),
  openPaperTicket: (paperSide) => set({ paperSide, paperTicketOpen: true }),
  closePaperTicket: () => set({ paperTicketOpen: false }),
  rememberWallet: (lastWallet) => set({ lastWallet }),
  rememberCreator: (lastCreator) => set({ lastCreator }),
}));
