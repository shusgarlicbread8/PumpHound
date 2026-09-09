import {
  API_BASE,
  type ApiErrorBody,
  type CandlesResponse,
  type CandidateDossier,
  type CandidateEnrichmentStatus,
  type Capabilities,
  type CreatorCoordination,
  type CreatorView,
  type EvidenceRecord,
  type EnrichedWallet,
  type FeatureVector,
  type GraphResponse,
  type HealthResponse,
  type HoldersResponse,
  type Investigation,
  type LaunchDetail,
  type DecisionsResponse,
  type LaunchesSnapshot,
  type ModelsResponse,
  type PaperOrderAck,
  type PredictionsResponse,
  type PaperOrderRequest,
  type PaperPortfolio,
  type PaperPortfolioSnapshot,
  type PaperQuote,
  type ResearchFunnelResponse,
  type ResearchLearningResponse,
  type ResearchStrategiesResponse,
  type SearchHit,
  type SettingsView,
  type SystemStatus,
  type TradesResponse,
  type WalletActivityResponse,
  type WalletFundingResponse,
  type WalletView,
  type Watchlist,
} from "./types";
import { asPaperPortfolio } from "./portfolioView";

export class ApiRequestError extends Error {
  readonly status: number;
  readonly body: ApiErrorBody | null;
  constructor(message: string, status: number, body: ApiErrorBody | null) {
    super(message);
    this.name = "ApiRequestError";
    this.status = status;
    this.body = body;
  }
}

async function parseJson<T>(res: Response): Promise<T> {
  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text) as unknown;
    } catch {
      throw new ApiRequestError("invalid JSON from API", res.status, null);
    }
  }
  if (!res.ok) {
    const body = data && typeof data === "object" && "error" in data ? (data as ApiErrorBody) : null;
    throw new ApiRequestError(body?.error.message ?? `HTTP ${res.status}`, res.status, body);
  }
  return data as T;
}

function qs(params?: Record<string, string | number | undefined | null>): string {
  if (!params) return "";
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === "") continue;
    u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : "";
}

export async function apiGet<T>(path: string, params?: Record<string, string | number | undefined | null>): Promise<T> {
  const res = await fetch(`${API_BASE}${path}${qs(params)}`, {
    method: "GET",
    headers: { Accept: "application/json" },
  });
  return parseJson<T>(res);
}

export async function apiSend<T>(
  path: string,
  method: "POST" | "PATCH" | "PUT" | "DELETE",
  body?: unknown,
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method,
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return parseJson<T>(res);
}

export const api = {
  health: () => apiGet<HealthResponse>("/health"),
  home: () => apiGet<Record<string, unknown>>("/home"),
  readiness: () => apiGet<Record<string, unknown>>("/system/readiness"),
  hotSet: () => apiGet<Record<string, unknown>>("/hot-set"),
  systemStatus: () => apiGet<SystemStatus>("/system/status"),
  status: () => apiGet<SystemStatus>("/system/status"),
  capabilities: () => apiGet<Capabilities>("/system/capabilities"),
  search: (q: string) => apiGet<{ items: SearchHit[] }>("/search", { q }),
  settings: () => apiGet<SettingsView>("/settings"),
  patchSettings: (value: Record<string, unknown>) => apiSend<SettingsView>("/settings", "PATCH", value),

  launches: (params?: Record<string, string | number | undefined | null>) =>
    apiGet<LaunchesSnapshot>("/launches", params),
  launch: (id: string) => apiGet<LaunchDetail>(`/launches/${encodeURIComponent(id)}`),
  trades: (id: string) => apiGet<TradesResponse>(`/launches/${encodeURIComponent(id)}/trades`),
  candles: (id: string, interval?: string) =>
    apiGet<CandlesResponse>(`/launches/${encodeURIComponent(id)}/candles`, { interval }),
  holders: (id: string) => apiGet<HoldersResponse>(`/launches/${encodeURIComponent(id)}/holders`),
  features: (id: string) => apiGet<FeatureVector>(`/launches/${encodeURIComponent(id)}/features`),
  predictions: (id: string) =>
    apiGet<PredictionsResponse>(`/launches/${encodeURIComponent(id)}/predictions`),
  decisions: (id: string) => apiGet<DecisionsResponse>(`/launches/${encodeURIComponent(id)}/decisions`),
  launchGraph: (id: string) => apiGet<GraphResponse>(`/launches/${encodeURIComponent(id)}/graph`),
  launchInvestigations: (id: string) =>
    apiGet<{ items: Investigation[] }>(`/launches/${encodeURIComponent(id)}/investigations`),
  launchIntelligence: (id: string) =>
    apiGet<Record<string, unknown>>(`/launches/${encodeURIComponent(id)}/intelligence`),

  wallets: () => apiGet<{ items: Record<string, unknown>[] }>("/wallets"),
  wallet: (address: string) => apiGet<WalletView>(`/wallets/${encodeURIComponent(address)}`),
  walletActivity: (address: string) =>
    apiGet<WalletActivityResponse>(`/wallets/${encodeURIComponent(address)}/activity`),
  walletFunding: (address: string) =>
    apiGet<WalletFundingResponse>(`/wallets/${encodeURIComponent(address)}/funding`),
  creators: () => apiGet<{ items: Record<string, unknown>[] }>("/creators"),
  creator: (address: string) => apiGet<CreatorView>(`/creators/${encodeURIComponent(address)}`),
  creatorCoordination: (address: string) =>
    apiGet<CreatorCoordination>(`/creators/${encodeURIComponent(address)}/coordination`),
  evidence: (id: string) => apiGet<EvidenceRecord>(`/evidence/${encodeURIComponent(id)}`),
  enrichmentStatus: () => apiGet<CandidateEnrichmentStatus>("/enrichment/status"),
  candidateDossier: (decisionId: string) =>
    apiGet<CandidateDossier>(`/enrichment/candidates/${encodeURIComponent(decisionId)}`),
  enrichedWallet: (wallet: string) =>
    apiGet<EnrichedWallet>(`/enrichment/wallets/${encodeURIComponent(wallet)}`),

  paperTruth: (filters: Record<string, string | undefined> = {}) => apiGet<unknown>("/paper/truth", filters),
  paperTrace: (id: string, at?: string) => apiGet<Record<string, unknown>>(`/paper/traces/${encodeURIComponent(id)}`, { at }),

  paperQuote: (req: PaperOrderRequest) => apiSend<PaperQuote>("/paper/quotes", "POST", req),
  paperOrder: (req: PaperOrderRequest) => apiSend<PaperOrderAck>("/paper/orders", "POST", req),
  cancelOrder: (id: string) => apiSend<{ order_id: string; status: string }>(`/paper/orders/${encodeURIComponent(id)}/cancel`, "POST", {}),
  portfolios: () => apiGet<{ items: PaperPortfolio[] }>("/paper/portfolios"),
  createPortfolio: (name: string) => apiSend<PaperPortfolio>("/paper/portfolios", "POST", { name }),
  portfolio: async (id: string) =>
    asPaperPortfolio(await apiGet<unknown>(`/paper/portfolios/${encodeURIComponent(id)}`)),
  portfolioSnapshot: (id: string) =>
    apiGet<PaperPortfolioSnapshot>(`/paper/portfolios/${encodeURIComponent(id)}/snapshot`),
  ledger: (id: string) =>
    apiGet<{ portfolio_id: string; items: unknown[]; reconciled?: boolean }>(
      `/paper/portfolios/${encodeURIComponent(id)}/ledger`,
    ),
  performance: (id: string) =>
    apiGet<{ portfolio_id: string; gross: string; net: string }>(
      `/paper/portfolios/${encodeURIComponent(id)}/performance`,
    ),
  breakerReset: (id: string, reason: string) =>
    apiSend<{ ok: boolean; audited: boolean }>(
      `/paper/portfolios/${encodeURIComponent(id)}/breaker-reset`,
      "POST",
      { reason, client_request_id: crypto.randomUUID() },
    ),

  modelTruth: () => apiGet<Record<string, unknown>>("/models/truth"),
  modelStates: (id: string) => apiGet<Record<string, unknown>>(`/launches/${encodeURIComponent(id)}/model-states`),
  championFunnel: () => apiGet<Record<string, unknown>>("/paper/champion-funnel"),
  models: () => apiGet<ModelsResponse>("/models"),
  researchStrategies: () => apiGet<ResearchStrategiesResponse>("/research/strategies"),
  researchFunnel: () => apiGet<ResearchFunnelResponse>("/research/funnel"),
  researchTimelines: () => apiGet<Record<string, unknown>>("/research/timelines"),
  researchLearning: (experimentId = "ALPHA-LEARN") =>
    apiGet<ResearchLearningResponse>("/research/learning", { experiment_id: experimentId }),
  watchlists: () => apiGet<{ items: Watchlist[] }>("/watchlists"),
  createWatchlist: (name: string) => apiSend<Watchlist>("/watchlists", "POST", { name }),
  addWatch: (id: string, subject: string) =>
    apiSend<{ ok: boolean }>(`/watchlists/${encodeURIComponent(id)}/items/${encodeURIComponent(subject)}`, "PUT", {
      subject_type: "launch",
    }),
  removeWatch: (id: string, subject: string) =>
    apiSend<{ ok: boolean }>(`/watchlists/${encodeURIComponent(id)}/items/${encodeURIComponent(subject)}`, "DELETE"),

  startReplay: (body: Record<string, unknown>) =>
    apiSend<{ run_id: string; state: string }>(`/replays`, "POST", body),
  replay: (id: string) => apiGet<{ run_id: string; state: string }>(`/replays/${encodeURIComponent(id)}`),
  controlReplay: (id: string, control: Record<string, unknown>) =>
    apiSend<{ run_id: string; control: unknown }>(`/replays/${encodeURIComponent(id)}/control`, "POST", control),

  investigate: (body: Record<string, unknown>) =>
    apiSend<{ investigation_id: string; state: string }>(`/investigations`, "POST", body),
  investigation: (id: string) => apiGet<Record<string, unknown>>(`/investigations/${encodeURIComponent(id)}`),

  alerts: () => apiGet<{ items: unknown[] }>("/alerts"),
};
