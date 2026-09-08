# Data Sources and Protocol Adapters

## 1. Source-of-truth hierarchy

1. Solana transaction/account/slot data
2. Official Pump/PumpSwap program IDLs and on-chain config accounts
3. Independent market-data APIs used as enrichment/cross-checks
4. Metadata URI and optional social data
5. LLM interpretation

An enrichment mismatch never overwrites the on-chain reconstruction. Store both values, their source, observed time and discrepancy.

## 2. Official Pump programs

At the time of this design:

- Pump bonding-curve program: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- PumpSwap program: `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA`

These are documented in Pump’s official public repository. The coding agent must vendor:

- `idl/pump.json`
- `idl/pump_amm.json`
- `idl/pump_fees.json`

into `vendor/pump-idl/` and create `SOURCE.lock` containing repository URL, fetched commit SHA, fetch time and SHA-256 for each file. Never track the mutable `main` branch silently.

On startup:

1. Read program address from each vendored IDL.
2. Assert it matches configured allowed program IDs.
3. Decode current global/config accounts.
4. Persist a `protocol_config_snapshot`.
5. Refuse to compute executable quotes if the observed account layout/discriminator is unknown.
6. Continue raw capture and mark derived state `decoder_incompatible`.

### Pump bonding curve events

Decode at minimum:

- `CreateEvent`
- `TradeEvent`
- `CompleteEvent`
- `CompletePumpAmmMigrationEvent`
- creator migration/set events
- parameter/config update events

Current `TradeEvent` includes mint, quote/base amounts, side, user, timestamp, reserve states, fee fields, creator, instruction name, mayhem/cashback/buyback data and quote mint. Do not derive these from log strings when the Anchor event payload is available.

Bind each decoded log/event to the verified active program invocation/CPI path. A matching discriminator or a copied `Program data` string without the correct program execution context is not trusted protocol evidence.

### PumpSwap events

Decode at minimum:

- `CreatePoolEvent`
- `BuyEvent`
- `SellEvent`
- deposit/withdraw events
- coin-creator update/migration events
- fee/config update events

PumpSwap effective quote reserves are:

```text
effective_quote_reserves =
    pool_quote_token_account.amount + Pool.virtual_quote_reserves
```

Use effective reserves in price, quote, index and paper-fill logic even while virtual quote reserves are zero for ordinary pools.

### SOL and USDC pairs

The adapter must treat the quote mint as data, not assume every legacy field called `sol_amount` is always native SOL. Normalize into `quote_amount_atomic` and retain original decoded fields for audit.

## 3. Primary real-time stream

### Helius LaserStream gRPC

Use Yellowstone-compatible gRPC subscriptions for:

- transactions mentioning Pump and PumpSwap program IDs;
- account updates owned by those programs when required;
- slot notifications;
- block metadata/transaction index required for deterministic ordering.

Default live commitment:

- receive and show `processed` immediately;
- reconcile to `confirmed`;
- mark durable labels/final accounting from `finalized` where required.

Store the last completed slot and canonical event cursor. On reconnect, request historical replay from that cursor when the provider supports it. Even if the provider advertises exactly-once delivery, PumpHound remains idempotent at storage boundaries.

Filters must exclude vote transactions and may include failed transactions in a separate raw stream because failed swaps help estimate landing/friction but are not economic trades.

### Standard Solana fallback

If LaserStream credentials are absent:

- use WebSocket `logsSubscribe` for each program;
- retrieve full transactions by signature through RPC;
- use `programSubscribe` for relevant account state;
- use `slotSubscribe`/`slotsUpdatesSubscribe`;
- de-duplicate and sequence after fetch.

This fallback is suitable for local product development and monitoring, not millisecond-sensitive execution. The System screen must show `RPC FALLBACK` and its measured delay.

### Replay source

`ParquetReplaySource` emits the same `RawChainEnvelope` contract with a virtual clock. It must support:

- exact original observed-time playback;
- accelerated playback;
- deterministic ordering by slot, transaction index, instruction index and event index;
- injected disconnect/gap scenarios;
- optional Monte Carlo latency applied only after event reconstruction.

## 4. Historical backfill

Use Helius `getTransactionsForAddress`/archival RPC for targeted address/program/token histories. Backfill is a resumable job with:

- query identity/hash;
- source and API version;
- cursor;
- requested slot/time bounds;
- received count;
- retry/rate-limit state;
- coverage report.

Backfill priorities:

1. Pump/PumpSwap transactions for the recent training period.
2. Creator histories for observed launches.
3. First-buyer wallet Pump-related histories.
4. Funding transfers for candidate wallets.
5. Broader wallet histories only for high-materiality cohorts.

Do not attempt to crawl every Solana wallet recursively. Apply budgets:

- maximum hops;
- maximum transactions per wallet;
- maximum high-degree counterparties;
- maximum historical age;
- relevance score threshold.

For very large historical research, support import from an archive dataset such as Triton One’s Old Faithful rather than hammering address APIs. Archive import still normalizes through the same decoder and canonical event IDs.

## 5. Enrichment adapters

All adapters implement:

```rust
#[async_trait]
pub trait EnrichmentProvider {
    fn name(&self) -> &'static str;
    async fn enrich(&self, request: EnrichmentRequest)
        -> Result<ProviderObservation, ProviderError>;
}
```

`ProviderObservation` includes payload hash, source timestamp, observed time, expiry, confidence/quality and normalized fields.

### Helius enhanced history/transfers

Use for wallet histories, funding trails and reconciliation. Cache by address + cursor/range. Do not make these calls in the critical per-trade reducer path.

### Birdeye

Optional cross-check/enrichment for:

- token/pair overview;
- price/OHLCV;
- liquidity;
- holder distribution;
- wallet P&L;
- token/pair transactions;
- new listing and WebSocket feeds.

Birdeye never replaces PumpHound’s raw trade ledger. Record rate limits and plan entitlements at startup.

### Jupiter Swap V2

Use `/swap/v2/build` in quote-only mode for executable cross-venue route checks after a token is routable. Do not sign or submit. Record:

- input/output mint and amount;
- out amount and threshold;
- route plan;
- price impact;
- context/blockhash metadata;
- API-observed time.

The current Jupiter docs say Swap V2 is the supported unified API and legacy Metis/Ultra endpoints are superseded. The adapter must therefore use only V2 contracts.

### DexScreener

Optional pair discovery/market cross-check, never the launch detector. Respect published rate limits and retain response timestamps.

### Pyth

Use Pyth Core/Hermes for SOL/USD valuation if a key is configured; otherwise derive a visibly lower-quality reference from a configured provider. Since Hermes access requirements can change, keep the endpoint/key server-side and expose only normalized `ReferencePrice`.

### Token metadata

On CreateEvent:

1. Store URI exactly.
2. Fetch over HTTPS/IPFS gateway with strict timeout and size/content-type limits.
3. Hash raw response.
4. Extract name, symbol, description, image, website and social links.
5. Proxy/cache images through backend; the webview never fetches arbitrary metadata URLs directly.
6. Sanitize all text/URLs; never execute HTML/scripts.

## 6. Funding and transfer collection

For a candidate buyer/creator:

1. Identify first observed funding needed to participate.
2. Fetch native SOL and stable-token transfers in bounded history.
3. Emit factual `FUNDED_BY` edges with transaction signature, slot, asset and amount.
4. Identify known labels only through the versioned `wallet_labels` registry.
5. Tag very high-degree sources as exchange/bridge/airdrop/unknown hub when evidence supports it.
6. Never infer same ownership from a single common exchange withdrawal.

## 7. Holder snapshots and historical balances

Trade events alone do not give complete ownership: SPL transfers can happen without invoking Pump. For a selected/candidate mint, obtain a slot-context holder/account snapshot through an entitled provider or standard token-account enumeration where supported, buffer concurrent updates, then replay updates after the snapshot boundary.

Aggregate token accounts by owner, excluding identified vaults/pools/burn/lock accounts as appropriate. Account layouts differ for Token-2022, so do not apply legacy fixed-size filters blindly. A top-account endpoint is not a complete holder census, and many accounts can belong to one owner.

Standard RPC current account reads are not arbitrary historical account-state reads. A historical holder/balance feature requires a valid boundary snapshot plus complete relevant transfers/account changes, or an explicitly supported historical provider. Otherwise return a covered-subset estimate/unknown and gate policies that require exact concentration.

Prioritize this work for inspected/high-ranked launches under a configured credit/account budget. Initial default: at most 50 concurrent deeply tracked mints, with eviction only after no open paper position and no pending label dependency. Retain eviction/coverage events. Discovery and raw program capture continue for the broad configured scope.

## 8. Data quality

Each market has component statuses:

| Component | Freshness target | Failure effect |
|---|---:|---|
| Chain stream | <2 s processed lag | Freeze new decisions |
| Canonical decoder | same event loop | Freeze quote/features for affected protocol |
| Market state | <1 s behind canonical event | Freeze paper orders |
| Wallet graph online | <10 s | Graph features unknown or stale |
| Historical profiles | <24 h | Downweight history features |
| Model service | <100 ms typical | Fall back to named heuristic or watch-only |
| USD reference | <30 s | Quote-native P&L only |

`DataQuality` is computed, persisted and carried in every snapshot/prediction. Policy rules can require a minimum quality grade.

## 9. Maximize observable network evidence, not imagined visibility

The decoder should retain additional high-value Solana context:

- fee payer, signers, program IDs and loaded address-table keys;
- inner CPI routes and transaction-level grouping;
- pre/post SOL and token balances with owner resolution;
- compute-unit limit/price, consumed units and transaction error class when available;
- transaction index within a slot, account-write versions and market-account contention proxies;
- token-program ownership, mint/freeze authorities and Token-2022 extensions;
- creator-fee recipients, claim events and fund-distribution paths;
- pool deposits/withdrawals, LP-token movements and verified lock/burn evidence where applicable;
- cross-venue movement and atomic arbitrage route patterns for supported decoders.

Do not assume a globally visible public mempool. Simultaneous trades or same-slot transactions do not prove a Jito bundle, common ownership, privileged launch access or malicious intent. Jito membership/ordering must be directly observed through an entitled source or remain a hypothesis. Program-owned vault movements are not independent buyers, and a burned LP token does not remove all token/creator/market risks.

The base stream covers the selected programs, not every funding transfer on the network. Wallet enrichment must use explicitly subscribed accounts or bounded history queries. Show coverage so the terminal never claims full-network omniscience.

## 10. Primary source anchors

- [Pump official IDLs and program docs](https://github.com/pump-fun/pump-public-docs)
- [Pump fee schedule](https://pump.fun/docs/fees)
- [Pump bonding curve](https://pump.fun/docs/bonding-curve)
- [Solana WebSocket methods](https://solana.com/docs/rpc/websocket)
- [Helius LaserStream](https://www.helius.dev/docs/laserstream)
- [Helius history](https://www.helius.dev/docs/rpc/historical-data)
- [Jupiter Swap V2](https://developers.jup.ag/docs/swap/index)
- [Yellowstone gRPC](https://github.com/rpcpool/yellowstone-grpc)
