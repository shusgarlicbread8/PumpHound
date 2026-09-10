import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { CreatorIndex, CreatorPage } from "../features/creators/CreatorPage";
import { GraphPage } from "../features/graph/GraphPage";
import { HomePage } from "../features/home/HomePage";
import { ModelsPage } from "../features/models/ModelsPage";
import { PaperPage } from "../features/paper/PaperPage";
import { PortfolioPage, PortfolioPositionPage } from "../features/portfolio/PortfolioPage";
import { RadarPage } from "../features/radar/RadarPage";
import { ReplayPage } from "../features/replay/ReplayPage";
import { ResearchPage } from "../features/research/ResearchPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { StrategiesPage } from "../features/strategies/StrategiesPage";
import { SystemPage } from "../features/system/SystemPage";
import { HyperliquidMarketPage, HyperliquidPage } from "../features/hyperliquid/HyperliquidPage";
import { OstiumPage } from "../features/ostium/OstiumPage";
import { PredictionPage } from "../features/prediction/PredictionPage";
import { TokenIndex } from "../features/token/TokenIndex";
import { TokenPage } from "../features/token/TokenPage";
import { TradePage } from "../features/trade/TradePage";
import { WalletIndex, WalletPage } from "../features/wallets/WalletPage";
import { truthFilters } from "../lib/productTruth";
import { Shell } from "./Shell";

const rootRoute = createRootRoute({
  component: Shell,
});

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomePage,
});

const radarRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/radar",
  component: RadarPage,
});

const tokenIndexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/token",
  component: TokenIndex,
});

const tokenRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/token/$launchId",
  component: TokenPage,
});

const graphRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/graph",
  component: GraphPage,
});

const walletsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/wallets",
  component: WalletIndex,
});

const walletRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/wallets/$address",
  component: WalletPage,
});

const creatorsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/creators",
  component: CreatorIndex,
});

const creatorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/creators/$address",
  component: CreatorPage,
});

const tradeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/trade",
  validateSearch: truthFilters,
  component: TradePage,
});

const paperRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/paper",
  component: PaperPage,
});

const portfolioRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portfolio",
  component: PortfolioPage,
});

const portfolioPositionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portfolio/$portfolioId/positions/$positionId",
  component: PortfolioPositionPage,
});

const strategiesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/strategies",
  component: StrategiesPage,
});

const researchRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/research",
  component: ResearchPage,
});

const replayRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/replay",
  validateSearch: (search: Record<string, unknown>) => ({ trace: typeof search.trace === "string" ? search.trace : undefined, mode: typeof search.mode === "string" ? search.mode : undefined }),
  component: ReplayPage,
});

const modelsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/models",
  component: ModelsPage,
});

const systemRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/system",
  component: SystemPage,
});

const hyperliquidRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/hyperliquid",
  component: HyperliquidPage,
});

const hyperliquidMarketRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/hyperliquid/$coin",
  component: HyperliquidMarketPage,
});

const ostiumRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/ostium",
  component: OstiumPage,
});

const predictionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/prediction",
  component: PredictionPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

const routeTree = rootRoute.addChildren([
  homeRoute,
  radarRoute,
  tokenIndexRoute,
  tokenRoute,
  graphRoute,
  walletsRoute,
  walletRoute,
  creatorsRoute,
  creatorRoute,
  tradeRoute,
  paperRoute,
  portfolioRoute,
  portfolioPositionRoute,
  strategiesRoute,
  researchRoute,
  replayRoute,
  modelsRoute,
  systemRoute,
  settingsRoute,
  hyperliquidRoute,
  hyperliquidMarketRoute,
  ostiumRoute,
  predictionRoute,
]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
