# Product and UX Specification

## 1. Design direction

PumpHound should feel like a purpose-built research terminal: fast, dark, compact, inspectable and alive. It must not resemble a generic SaaS dashboard, neon casino, Pump.fun clone or AI chat wrapper.

Brand idea: a hound follows evidence trails. The visual motif is a restrained “scent trail” line used for causal/evidence paths—not dog illustrations everywhere.

### Visual tokens

| Token | Value | Use |
|---|---|---|
| Canvas | `#090B0E` | Main background |
| Surface 1 | `#0F1318` | Panels |
| Surface 2 | `#151B22` | Raised/hover panels |
| Hairline | `#26303A` | Borders and grid |
| Primary text | `#E7EDF3` | Titles and values |
| Secondary text | `#8F9BA8` | Labels |
| Hound amber | `#F3A83B` | Selection, brand, evidence trail |
| Positive | `#46D39A` | Positive change/low risk |
| Negative | `#FF667A` | Loss/high risk |
| Information | `#63A7FF` | Links, neutral model info |
| Unknown | `#8B7E9E` | Missing/uncertain evidence |

Typography:

- UI: Inter Variable or system `-apple-system`.
- Numeric: IBM Plex Mono.
- 12 px labels, 13–14 px body, 16 px panel headers, 24–30 px primary values.
- Tabular numerals for every market number.

Spacing is based on 4 px. Default panel padding 12 px. Corner radius 6 px; avoid oversized cards and pills.

## 2. Application shell

The window has four persistent regions:

1. **Top status rail, 32 px:** mode, chain health, current slot, provider lag, paper NAV, global command button.
2. **Left navigation, 56 px collapsed / 184 px expanded:** Radar, Token, Graph, Wallets, Creators, Paper, Replay, Models, System, Settings.
3. **Main workspace:** resizable panels and tabs.
4. **Bottom event rail, 24 px:** messages/sec, queue depth, last persisted event, data-quality warnings.

Keyboard:

- `⌘K` opens command palette.
- `/` focuses global token/wallet/signature search.
- `G R` Radar, `G P` Paper, `G M` Models, `G S` System.
- `Space` pauses/restarts replay only.
- `B` opens paper-buy ticket for selected token.
- `S` opens paper-sell ticket for selected position.
- `Esc` closes the top drawer/modal.

All shortcuts are disabled while typing in an input.

## 3. Onboarding

One compact setup screen appears only until a working data source is chosen.

Steps:

1. Select `Live Helius`, `Live RPC Fallback` or `Bundled Replay`.
2. For Helius, enter API key into the native secret-store flow; never echo it.
3. Run connection check: RPC slot, stream connection, ClickHouse/Postgres/NATS health.
4. Choose paper account:
   - base currency USD;
   - initial NAV $1,000;
   - starting balances default 50% of budget in SOL and 50% in USDC, converted with the chosen as-of reference prices, never a fixed SOL quantity that can exceed the budget;
   - when USD references are absent, offer explicit quote-native paper balances and mark consolidated USD NAV unavailable.
5. Open Radar.

There is no account creation or cloud login in local v1.

## 4. Radar screen

### Layout

- Left 68%: virtualized launch table.
- Right 32%: selected-token “fast inspect” panel.
- Top filter bar remains sticky.

### Columns

| Column | Content |
|---|---|
| Age | seconds/minutes since CreateEvent |
| Token | image, symbol, truncated mint, quote asset |
| Venue | Curve / PumpSwap |
| Price | quote and USD if available |
| MC | explicit estimated market cap |
| Curve | progress bar or “Graduated” |
| Buyers | unique / retained |
| Flow | net quote flow and 30 s acceleration |
| Cluster | largest connected cohort share |
| Creator | history badge |
| Opportunity | versioned 0–100 rank plus expected net return when a model exists |
| Rug-like risk | operationally defined probability with horizon, unavailable before training |
| State | `WATCH`, `PAPER BUY`, `AVOID`, held |

Rows flash for 400 ms on meaningful state changes, not every tick. Sorting a live column pins the sort and buffers new rows in a `N new` control so the list does not jump.

### Filters

- launch age;
- venue/lifecycle;
- SOL or USDC quote;
- minimum buyers/volume/liquidity;
- curve progress;
- model-qualified only;
- maximum rug probability;
- minimum organic score;
- exclude unknown data quality;
- creator history;
- cluster concentration;
- currently held/watchlisted.

Saved filter sets are local configurations, not hard-coded strategies.

### Fast inspect

Shows 60-second price sparkline, curve/reserves, top six evidence flags, first-buyer diversity, creator summary, recent large trades and buttons:

- Open full token
- Add/remove watchlist
- Open paper ticket
- Run investigator

## 5. Token workspace

Header:

- token image/name/symbol/mint copy;
- mode and venue;
- age, quote mint and migration status;
- price, executable paper quote, market cap, curve progress;
- paper position if held;
- data-quality chip.

Six score tiles sit below. Clicking a tile opens `ScoreDetailsDrawer` with:

- exact definition;
- observation and prediction horizons;
- current value and calibration band;
- top positive/negative features;
- model and feature-set versions;
- evidence links;
- value history;
- missing inputs.

### Tabs

#### Overview

- Lightweight Charts price/candles with buy/sell markers and prediction changes.
- Volume/flow pane.
- Curve/pool reserves and estimated slippage ladder for $10/$25/$50/$100.
- Buyer-retention funnel: entered → still holding after 30 s/2 m/5 m.
- Evidence timeline combining launches, cohort arrivals, creator actions, large sells, migration and paper actions.

#### Tape

Virtualized trades with observed latency, slot/order index, wallet, side, quote/base amount, effective price, price impact, protocol fees, wallet/cluster badges and signature.

#### Graph

Focused Sigma.js graph centred on token. Default shows creator, first 100 material buyers, funding sources and known cohort links. Controls:

- factual / inferred edge toggle;
- evidence threshold;
- observation-time slider;
- edge-type filters;
- expand one hop;
- pin/compare nodes;
- “explain this connection.”

Never draw every edge by default. Aggregate low-materiality nodes into visible group nodes.

#### Holders

Point-in-time holder concentration, cluster-adjusted concentration, creator-connected ownership, entry cost estimates, realized/unrealized cohort P&L and change over time.

#### Creator

Creator address/history, prior launches, graduation/rug-like rates, median time-to-peak, creator-linked first buyers, fee activity and evidence timeline.

#### Paper

Order ticket, quote decomposition, open paper orders, fills, position lots, exit recommendations, P&L attribution and policy decisions.

#### Evidence

Chronological immutable evidence table. Each derived claim expands to source event IDs/signatures and computation version.

## 6. Wallet Intelligence

Global wallet search accepts an address, token mint or transaction signature.

Wallet page:

- balances/Pump-related positions;
- first/last observed, wallet age estimate and funding history;
- Pump/PumpSwap trading outcomes;
- realized P&L estimates with confidence and missing-cost warning;
- creator relationships;
- launch cohorts and repeat partners;
- cluster memberships by algorithm version;
- intent-hypothesis evidence scores; probabilities only for calibrated targets;
- timeline and graph.

The UI says `Evidence suggests coordinated participation`, never `same owner` unless the user has supplied a verified label.

## 7. Graph Explorer

Two-pane layout:

- left: WebGL graph;
- right: selected node/edge evidence inspector.

Modes:

- Token neighbourhood
- Wallet neighbourhood
- Creator network
- Persistent cohorts
- Funding flow
- Co-sell/dump rings

The temporal scrubber changes the graph as-of time. Cluster colouring always names the cluster algorithm/version. Edge thickness uses transformed weight, not raw transfer value. CEX/bridge/high-degree hubs can be hidden or downweighted.

## 8. Paper Desk

### Portfolio header

NAV, cash by quote mint, gross/net exposure, realised/unrealised P&L, session/daily drawdown, costs, number of positions and circuit-breaker state.

### Position table

Token, size, cost basis, executable exit value, P&L, age, max favourable/adverse excursion, current exit hazard, stop/trailing policy and action.

### Decision ledger

Every row contains:

- decision time and market snapshot;
- policy/model versions;
- action;
- expected net edge;
- reason and blocker codes;
- intended order;
- simulated outcome;
- link to evidence bundle.

### Paper ticket

Inputs: side, quote/base amount, policy/manual tag, slippage limit, latency profile. Preview returns:

- state as-of quote time;
- gross output;
- protocol/creator/LP fees;
- predicted network/priority/tip cost;
- price impact;
- min-out;
- estimated landing window;
- pessimistic/base/optimistic fill;
- new exposure and risk-limit impact.

Confirm button always reads `Place paper order`.

## 9. Replay

- Pick historical token/session/date.
- Select deterministic or Monte Carlo execution.
- Select model/policy versions.
- Speed: 0.25×, 1×, 10×, 100×, max.
- Scrub to an event boundary.
- Compare up to three policies side by side.

At any point “what was knowable?” opens a drawer listing exactly which sources and snapshots existed then.

## 10. Models

Model Registry view:

- status: candidate, shadow, champion, retired, rejected;
- target/horizon/observation window;
- train/validation/test periods;
- feature schema hash;
- PR-AUC, ROC-AUC, log loss, Brier, calibration error;
- paper EV after costs;
- max drawdown and top-winner removal results;
- drift and live calibration;
- promote/retire actions protected by confirmation.

Feature Explorer:

- distribution through time;
- missingness;
- train/live drift;
- SHAP aggregate and per-prediction explanation;
- leakage audit metadata.

## 11. System

One operational screen shows:

- chain slot and commitment lag;
- provider connection/reconnect history;
- messages/sec by type;
- last raw/canonical/persisted/published IDs;
- NATS consumer lag;
- ClickHouse/Postgres status;
- graph/feature/model worker freshness;
- orphan/fork reconciliation count;
- enrichment rate limits;
- disk use and retention;
- clock drift.

Red state must say what is stale and which product values are affected.

## 12. Empty, loading and error states

- Use skeletons only for the initial authoritative snapshot.
- During stream reconnect, keep last known values but overlay `STALE · last update 8.2 s`.
- Unknown features show an em dash plus reason tooltip.
- If no compatible model exists, probability tiles show `MODEL NOT TRAINED`; any heuristic rank is separately labelled `HEURISTIC` with its rule version.
- If an image URI fails, show generated initials; never block a token row.
- Errors appear in the relevant panel plus System log, not as repeated global toasts.

## 13. Accessibility and performance

- Full keyboard navigation and visible focus ring.
- Text/colour contrast meets WCAG AA.
- Colour is never the only risk cue; include icon and label.
- Virtualize Radar, Tape, Evidence, Positions and logs.
- Batch UI deltas at 100–250 ms; do not rerender the graph/chart per transaction.
- Keep interaction latency below 100 ms at the 95th percentile on the target Mac.
