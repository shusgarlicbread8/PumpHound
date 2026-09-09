//! Immutable execution contracts shared by the paper loop, the ledger, replay and research.
//!
//! These are the ONLY definitions of `ExecutionIntent` / `ExecutionResult` in the workspace.
//! Every economic field is an exact integer (lamports / atomic units / bps). Floats never enter
//! ledger math; they appear only in derived, clearly-labelled diagnostics.

use pumphound_domain::{Protocol, Pubkey, Slot, Timestamp, TradeSide};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bump on any change that can alter a fill, a fee, or a failure outcome for the same inputs.
pub const SIMULATOR_VERSION: &str = "execution-twin-v1.0.0";

/// Upstream artifacts the twin is derived from. Recorded on every result for reproducibility.
pub const UPSTREAM_PUMP_PUBLIC_DOCS_COMMIT: &str = "9c82f61cb711b044a17f770ab8ce9f9bdf78f333";
pub const UPSTREAM_PUMP_SDK_VERSION: &str = "@pump-fun/pump-sdk@1.36.0";
pub const UPSTREAM_PUMP_SWAP_SDK_VERSION: &str = "@pump-fun/pump-swap-sdk@1.19.0";

/// A frozen request to execute exactly one swap. Produced by a strategy decision, never mutated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionIntent {
    pub trace_id: Uuid,
    pub intent_id: Uuid,
    pub experiment_id: String,
    pub strategy_version: String,
    pub decision_id: Uuid,
    pub decision_timestamp: Timestamp,
    pub decision_slot: Slot,
    pub mint: Pubkey,
    pub market: Pubkey,
    pub venue: Protocol,
    pub side: TradeSide,
    pub quote_asset: Pubkey,
    /// Lamports (or quote atomic) the strategy is willing to spend on a BUY, including fees.
    pub requested_quote_amount: u64,
    /// Base atomic to sell. Zero for buys.
    pub token_amount_if_sell: u64,
    pub slippage_limit_bps: u32,
    /// Model versions consulted by the strategy (may be empty for heuristic/control books).
    pub strategy_model_versions: Vec<String>,
    pub market_snapshot_id: String,
    /// Which cost scenario the book runs under. Books in one experiment family must agree.
    pub cost_scenario: JitoScenario,
}

/// Tip environment assumption. Research reruns every strategy under all of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JitoScenario {
    #[default]
    NoJito,
    Conservative,
    Median,
    Aggressive,
}

impl JitoScenario {
    pub fn as_str(self) -> &'static str {
        match self {
            JitoScenario::NoJito => "NO_JITO",
            JitoScenario::Conservative => "CONSERVATIVE",
            JitoScenario::Median => "MEDIAN",
            JitoScenario::Aggressive => "AGGRESSIVE",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "NO_JITO" => Some(JitoScenario::NoJito),
            "CONSERVATIVE" => Some(JitoScenario::Conservative),
            "MEDIAN" => Some(JitoScenario::Median),
            "AGGRESSIVE" => Some(JitoScenario::Aggressive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionKind {
    PumpBuyExactSolIn,
    PumpSell,
    PumpSwapBuyExactQuoteIn,
    PumpSwapSell,
}

/// Why a simulated transaction did not deliver the requested swap.
///
/// Fee semantics: `charges_network_fee()` is true only when the transaction would have been
/// included in a block (base + priority fee are burned/paid even when the program errors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClass {
    StaleBlockhash,
    SlippageExceeded,
    InsufficientOutput,
    AccountStateChanged,
    ComputeExceeded,
    AccountContention,
    SimulationFailure,
    LandingTimeout,
    UnsupportedMarketState,
    DataStale,
    Unknown,
}

impl FailureClass {
    /// Included-but-failed transactions still pay base + priority fees. Never-landed ones do not.
    pub fn charges_network_fee(self) -> bool {
        match self {
            FailureClass::SlippageExceeded
            | FailureClass::InsufficientOutput
            | FailureClass::AccountStateChanged
            | FailureClass::ComputeExceeded => true,
            FailureClass::StaleBlockhash
            | FailureClass::AccountContention
            | FailureClass::SimulationFailure
            | FailureClass::LandingTimeout
            | FailureClass::UnsupportedMarketState
            | FailureClass::DataStale
            | FailureClass::Unknown => false,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            FailureClass::StaleBlockhash => "STALE_BLOCKHASH",
            FailureClass::SlippageExceeded => "SLIPPAGE_EXCEEDED",
            FailureClass::InsufficientOutput => "INSUFFICIENT_OUTPUT",
            FailureClass::AccountStateChanged => "ACCOUNT_STATE_CHANGED",
            FailureClass::ComputeExceeded => "COMPUTE_EXCEEDED",
            FailureClass::AccountContention => "ACCOUNT_CONTENTION",
            FailureClass::SimulationFailure => "SIMULATION_FAILURE",
            FailureClass::LandingTimeout => "LANDING_TIMEOUT",
            FailureClass::UnsupportedMarketState => "UNSUPPORTED_MARKET_STATE",
            FailureClass::DataStale => "DATA_STALE",
            FailureClass::Unknown => "UNKNOWN",
        }
    }
}

/// Exact protocol-side fee decomposition for one swap, in quote atomic units.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFees {
    pub protocol_fee_quote: u64,
    pub creator_fee_quote: u64,
    pub lp_fee_quote: u64,
    pub protocol_fee_bps: u64,
    pub creator_fee_bps: u64,
    pub lp_fee_bps: u64,
    /// Cashback coins: replaces the creator fee (creator_fee_bps becomes 0). Charged to the
    /// trader at execution; any later cashback claim is NOT credited to the paper ledger.
    #[serde(default)]
    pub cashback_fee_quote: u64,
    #[serde(default)]
    pub cashback_fee_bps: u64,
    /// Fee tier selected by market cap, or "flat"/"global" when tiers do not apply.
    pub fee_tier: String,
}

impl ProtocolFees {
    pub fn total_quote(&self) -> u64 {
        self.protocol_fee_quote
            .saturating_add(self.creator_fee_quote)
            .saturating_add(self.lp_fee_quote)
            .saturating_add(self.cashback_fee_quote)
    }
}

/// Everything Solana itself charges or that the modeled auction demands, in lamports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkCosts {
    pub base_fee_lamports: u64,
    pub compute_unit_limit: u32,
    pub compute_units_consumed: u32,
    /// micro-lamports per CU.
    pub compute_unit_price_micro_lamports: u64,
    pub priority_fee_lamports: u64,
    pub estimated_jito_tip_lamports: u64,
    /// One-off rent for accounts the transaction must create (ATA, volume accumulator).
    pub rent_lamports: u64,
    pub priority_fee_provider: String,
    pub jito_scenario: JitoScenario,
    pub jito_model_version: String,
}

impl NetworkCosts {
    /// Cost charged when the transaction lands (successfully or not).
    pub fn landed_cost_lamports(&self) -> u64 {
        self.base_fee_lamports
            .saturating_add(self.priority_fee_lamports)
            .saturating_add(self.estimated_jito_tip_lamports)
    }
    /// Total cost including rent that only applies to successful account creation.
    pub fn total_success_lamports(&self) -> u64 {
        self.landed_cost_lamports()
            .saturating_add(self.rent_lamports)
    }
}

/// Price triple that keeps adverse selection and price impact separate.
/// Prices are quote atomic per 1e6 base atomic (i.e. lamports per whole token), integer-scaled by 1e9
/// to preserve precision without floats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceTriple {
    pub decision_price_e9: u128,
    pub arrival_price_e9: u128,
    pub execution_price_e9: u128,
    pub post_cost_effective_price_e9: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingSample {
    pub landing_model_version: String,
    pub arrival_timestamp: Timestamp,
    pub arrival_slot: Slot,
    pub attempted_slot: Slot,
    pub landed_slot: Option<Slot>,
    /// slots between attempt and landing; None when never landed.
    pub landing_slots: Option<u32>,
    pub arrival_delay_ms: u64,
    pub p_land_current_slot_bps: u32,
    pub p_land_plus_1_bps: u32,
    pub p_land_plus_2_bps: u32,
    pub p_expire_bps: u32,
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub result_id: Uuid,
    pub trace_id: Uuid,
    pub intent_id: Uuid,
    pub simulator_version: String,
    pub transaction_kind: TransactionKind,
    pub constructed: bool,
    pub simulated: bool,
    pub landing: LandingSample,
    pub success: bool,
    pub failure_class: Option<FailureClass>,
    pub failure_detail: Option<String>,
    pub protocol_fees: ProtocolFees,
    pub network: NetworkCosts,
    /// Actually charged network lamports given landing/failure semantics.
    pub network_cost_charged_lamports: u64,
    pub price_impact_bps: u32,
    /// Signed: positive means arrival moved against the order relative to decision.
    pub adverse_move_bps: i32,
    pub requested_quote: u64,
    pub filled_quote: u64,
    pub filled_base: u64,
    pub minimum_output: u64,
    pub expected_output_at_decision: u64,
    pub prices: PriceTriple,
    pub decision_snapshot_id: String,
    pub arrival_snapshot_id: String,
    /// sha256 of the canonical transaction bytes the builder produced (no signature).
    pub transaction_sha256: String,
    pub evidence: serde_json::Value,
}

impl ExecutionResult {
    /// Total quote-denominated drag versus the decision quote: protocol fees, network costs,
    /// impact and adverse selection realized in the fill. Positive = cost.
    pub fn all_in_cost_lamports(&self) -> i128 {
        let protocol = self.protocol_fees.total_quote() as i128;
        let network = self.network_cost_charged_lamports as i128;
        protocol + network
    }
}

/// Stable hash of a result used by replay determinism tests.
pub fn result_fingerprint(result: &ExecutionResult) -> String {
    let canonical = serde_json::json!({
        "simulator_version": result.simulator_version,
        "transaction_kind": result.transaction_kind,
        "success": result.success,
        "failure_class": result.failure_class,
        "protocol_fees": result.protocol_fees,
        "network_cost_charged_lamports": result.network_cost_charged_lamports,
        "price_impact_bps": result.price_impact_bps,
        "adverse_move_bps": result.adverse_move_bps,
        "filled_quote": result.filled_quote,
        "filled_base": result.filled_base,
        "minimum_output": result.minimum_output,
        "prices": result.prices,
        "landing_slots": result.landing.landing_slots,
        "arrival_delay_ms": result.landing.arrival_delay_ms,
        "transaction_sha256": result.transaction_sha256,
    });
    pumphound_domain::sha256_hex(canonical.to_string().as_bytes())
}
