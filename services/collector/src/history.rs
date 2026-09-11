use anyhow::{bail, Context};
use chrono::{DateTime, TimeZone, Utc};
use futures::{stream, StreamExt};
use pumphound_chain_source::{parse_transaction, SourceConfig};
use pumphound_domain::{
    payload_sha256, Canonicality, ChainEvent, ChainEventPayload, Commitment, LaunchpadDecoder,
    Protocol, RawTransaction, DECODER_VERSION, PUMPSWAP_PROGRAM_ID, PUMP_PROGRAM_ID,
};
use pumphound_protocol_pump::PumpBondingCurveDecoder;
use pumphound_protocol_pumpswap::PumpSwapDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::warn;

const CHECKPOINT_VERSION: u32 = 2;
const MANIFEST_VERSION: u32 = 2;
const FULL_CREDIT_UPPER_BOUND_PER_CALL: u64 = 100;
const MAX_MANIFEST_FAILURE_GROUPS: usize = 200;
const PAID_CORPUS_PRE_SEMANTICS_HASH: &str =
    "1d7ea309cb394bc77afe872c572db0d325312eba8f0ce7e7d6ad20c15a6d10d2";
const COVERAGE_LIMITATION: &str = "Address history is bounded by the configured calendar \
windows, raw-transaction cap, request cap, and launch target. Helius getTransactionsForAddress \
improves address-history coverage but still cannot prove that every Pump/PumpSwap launch is \
discoverable. Provider retention, plan limits, malformed records, decoder failures, and \
transactions outside the selected windows remain explicit gaps.";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMode {
    HeliusFull,
    StandardRpc,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowStrategy {
    RoundRobin,
    Sequential,
}

fn default_window_strategy() -> WindowStrategy {
    WindowStrategy::RoundRobin
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeWindow {
    pub id: String,
    pub gte: DateTime<Utc>,
    pub lte: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    pub output_dir: String,
    #[serde(default)]
    pub history_mode: Option<HistoryMode>,
    pub windows: Vec<TimeWindow>,
    #[serde(default = "default_window_strategy")]
    pub window_strategy: WindowStrategy,
    #[serde(default = "default_target_launches")]
    pub target_launches: usize,
    pub max_raw_transactions: usize,
    pub max_requests: u64,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default)]
    pub min_slot: Option<u64>,
    #[serde(default)]
    pub max_slot: Option<u64>,
    #[serde(default = "default_rate")]
    pub rate_limit_per_second: f64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default)]
    pub max_supported_transaction_version: u8,
    #[serde(default = "default_commitment")]
    pub commitment: String,
    #[serde(default = "default_failure_limit")]
    pub max_failure_records: usize,
    #[serde(default = "default_fallback")]
    pub allow_standard_fallback_on_denial: bool,
    #[serde(default = "default_min_decode_success")]
    pub minimum_decode_success_fraction: f64,
    #[serde(default = "default_max_decode_failure")]
    pub maximum_decode_failure_fraction: f64,
    #[serde(default = "default_min_canonical")]
    pub minimum_canonical_events: usize,
    #[serde(default = "default_min_launches")]
    pub minimum_true_or_provisional_launches: usize,
    #[serde(default = "default_signature_examples")]
    pub failure_signature_examples: usize,
}

fn default_config_version() -> u32 {
    2
}
fn default_target_launches() -> usize {
    25_000
}
fn default_page_size() -> usize {
    1_000
}
fn default_rate() -> f64 {
    5.0
}
fn default_retries() -> u32 {
    5
}
fn default_backoff_ms() -> u64 {
    500
}
fn default_concurrency() -> usize {
    4
}
fn default_timeout() -> u64 {
    30
}
fn default_commitment() -> String {
    "finalized".into()
}
fn default_failure_limit() -> usize {
    1_000
}
fn default_fallback() -> bool {
    true
}
fn default_min_decode_success() -> f64 {
    0.90
}
fn default_max_decode_failure() -> f64 {
    0.10
}
fn default_min_canonical() -> usize {
    1
}
fn default_min_launches() -> usize {
    1
}
fn default_signature_examples() -> usize {
    10
}

impl HistoryConfig {
    pub fn load(path: &Path) -> anyhow::Result<(Self, String)> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("read history config {}", path.display()))?;
        let config: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse history config {} as JSON", path.display()))?;
        config.validate()?;
        Ok((
            config.clone(),
            payload_sha256(&serde_json::to_vec(&config)?),
        ))
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.version != 2 {
            bail!("unsupported history config version {}", self.version);
        }
        if self.output_dir.trim().is_empty() {
            bail!("output_dir must not be empty");
        }
        if self.windows.is_empty() {
            bail!("at least one chronological block-time window is required");
        }
        let mut ids = BTreeSet::new();
        let mut previous_start = None;
        for window in &self.windows {
            if window.id.trim().is_empty() || !ids.insert(window.id.clone()) {
                bail!("window IDs must be non-empty and unique");
            }
            if window.gte > window.lte {
                bail!("window {} has gte after lte", window.id);
            }
            if previous_start.is_some_and(|start| window.gte < start) {
                bail!("windows must be listed in chronological order");
            }
            previous_start = Some(window.gte);
        }
        if self.target_launches == 0 || self.max_raw_transactions == 0 || self.max_requests == 0 {
            bail!("target_launches, max_raw_transactions, and max_requests must be positive");
        }
        if !(1..=1_000).contains(&self.page_size) {
            bail!("page_size must be between 1 and 1000");
        }
        if self.concurrency == 0
            || self.request_timeout_secs == 0
            || !self.rate_limit_per_second.is_finite()
            || self.rate_limit_per_second <= 0.0
        {
            bail!("concurrency, timeout, and rate limit must be positive");
        }
        if self.commitment != "finalized" {
            bail!("historical corpus commitment must be finalized");
        }
        if self.min_slot.zip(self.max_slot).is_some_and(|(a, b)| a > b) {
            bail!("min_slot must be <= max_slot");
        }
        if !(0.0..=1.0).contains(&self.minimum_decode_success_fraction)
            || !(0.0..=1.0).contains(&self.maximum_decode_failure_fraction)
        {
            bail!("decode quality fractions must be between 0 and 1");
        }
        if self.failure_signature_examples == 0 {
            bail!("failure_signature_examples must be positive");
        }
        Ok(())
    }

    pub fn maximum_credit_estimate(&self) -> u64 {
        self.max_requests
            .saturating_mul(FULL_CREDIT_UPPER_BOUND_PER_CALL)
    }
}

#[derive(Serialize)]
struct LegacyHistoryConfig<'a> {
    version: u32,
    output_dir: &'a str,
    history_mode: Option<HistoryMode>,
    windows: &'a [TimeWindow],
    window_strategy: WindowStrategy,
    target_launches: usize,
    max_raw_transactions: usize,
    max_requests: u64,
    page_size: usize,
    before: &'a Option<String>,
    until: &'a Option<String>,
    min_slot: Option<u64>,
    max_slot: Option<u64>,
    rate_limit_per_second: f64,
    retries: u32,
    backoff_ms: u64,
    concurrency: usize,
    request_timeout_secs: u64,
    max_supported_transaction_version: u8,
    commitment: &'a str,
    max_failure_records: usize,
    allow_standard_fallback_on_denial: bool,
}

fn legacy_config_sha256(config: &HistoryConfig) -> anyhow::Result<String> {
    let legacy = LegacyHistoryConfig {
        version: config.version,
        output_dir: &config.output_dir,
        history_mode: config.history_mode,
        windows: &config.windows,
        window_strategy: config.window_strategy,
        target_launches: config.target_launches,
        max_raw_transactions: config.max_raw_transactions,
        max_requests: config.max_requests,
        page_size: config.page_size,
        before: &config.before,
        until: &config.until,
        min_slot: config.min_slot,
        max_slot: config.max_slot,
        rate_limit_per_second: config.rate_limit_per_second,
        retries: config.retries,
        backoff_ms: config.backoff_ms,
        concurrency: config.concurrency,
        request_timeout_secs: config.request_timeout_secs,
        max_supported_transaction_version: config.max_supported_transaction_version,
        commitment: &config.commitment,
        max_failure_records: config.max_failure_records,
        allow_standard_fallback_on_denial: config.allow_standard_fallback_on_denial,
    };
    Ok(payload_sha256(&serde_json::to_vec(&legacy)?))
}

#[derive(Serialize)]
struct SourceSemantics<'a> {
    programs: [&'static str; 2],
    requested_mode: Option<HistoryMode>,
    windows: &'a [TimeWindow],
    before: &'a Option<String>,
    until: &'a Option<String>,
    min_slot: Option<u64>,
    max_slot: Option<u64>,
    commitment: &'a str,
    transaction_details: &'static str,
    sort_order: &'static str,
    encoding: &'static str,
    max_supported_transaction_version: u8,
    status_filter: &'static str,
}

fn source_semantics_hash(config: &HistoryConfig) -> anyhow::Result<String> {
    Ok(payload_sha256(&serde_json::to_vec(&SourceSemantics {
        programs: [PUMP_PROGRAM_ID, PUMPSWAP_PROGRAM_ID],
        requested_mode: config.history_mode,
        windows: &config.windows,
        before: &config.before,
        until: &config.until,
        min_slot: config.min_slot,
        max_slot: config.max_slot,
        commitment: &config.commitment,
        transaction_details: "full",
        sort_order: "asc",
        encoding: "json",
        max_supported_transaction_version: config.max_supported_transaction_version,
        status_filter: "succeeded",
    })?))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SignatureInfo {
    signature: String,
    slot: u64,
    #[serde(rename = "blockTime")]
    block_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FailureRecord {
    pub program_id: String,
    pub window_id: String,
    pub signature: Option<String>,
    pub method: String,
    pub error: String,
    pub attempts: u32,
    pub retryable_on_resume: bool,
    #[serde(default)]
    pub transient: bool,
    #[serde(default)]
    transient_attempt_history: Vec<TransientAttempt>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RecoveredTransientError {
    pub method: String,
    pub error: String,
    pub count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PendingTransaction {
    info: SignatureInfo,
    failure: FailureRecord,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StreamCheckpoint {
    pub program_id: String,
    pub window_id: String,
    pub pagination_token: Option<String>,
    pub before: Option<String>,
    pub pages: u64,
    pub records_returned: u64,
    pub complete: bool,
    pub stop_reason: Option<String>,
    pub last_error: Option<FailureRecord>,
    #[serde(default)]
    pub recovered_transient_errors: BTreeMap<String, RecoveredTransientError>,
    pending_transactions: BTreeMap<String, PendingTransaction>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HistoryCheckpoint {
    version: u32,
    #[serde(alias = "config_sha256", alias = "run_config_sha256")]
    run_config_hash: String,
    #[serde(default)]
    source_semantics_hash: String,
    #[serde(default)]
    decoder_version: String,
    requested_mode: Option<HistoryMode>,
    effective_mode: HistoryMode,
    fallback_reason: Option<String>,
    total_requests: u64,
    helius_full_successful_requests: u64,
    helius_full_records_returned: u64,
    helius_metered_credits_estimate: u64,
    global_stop_reason: Option<String>,
    streams: BTreeMap<String, StreamCheckpoint>,
    decode_failures: BTreeMap<String, FailureRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DecodeStatus {
    Success,
    SchemaIncompatible,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DecodeIssue {
    program_id: String,
    method: String,
    error: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DecodeState {
    signature: String,
    raw_payload_sha256: String,
    decoder_version: String,
    status: DecodeStatus,
    canonical_event_count: u64,
    quarantine_count: u64,
    event_ids: Vec<String>,
    issues: Vec<DecodeIssue>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CorpusHashes {
    pub run_config_hash: String,
    pub source_semantics_hash: String,
    pub checkpoint_sha256: String,
    pub raw_index_sha256: String,
    pub canonical_jsonl_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreditAccounting {
    pub pricing_basis: String,
    pub successful_full_requests: u64,
    pub full_records_returned: u64,
    pub metered_credits_estimate: u64,
    pub conservative_credits_per_request: u64,
    pub conservative_credits_upper_bound_used: u64,
    pub configured_maximum_credit_estimate: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalendarCoverage {
    pub configured_window_count: usize,
    pub configured_complete_days: u64,
    pub earliest_gte: DateTime<Utc>,
    pub latest_lte: DateTime<Utc>,
    pub completed_windows: Vec<String>,
    pub incomplete_windows: Vec<String>,
    pub observed_calendar_days: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Completeness {
    pub status: String,
    pub bounded_scan_complete: bool,
    pub target_launches: usize,
    pub target_reached: bool,
    pub raw_bound_reached: bool,
    pub request_bound_reached: bool,
    pub all_launches_complete: bool,
    pub coverage_limitation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FailureAggregate {
    pub category: String,
    pub method: String,
    pub error: String,
    pub count: u64,
    pub signature_examples: Vec<String>,
    pub program_ids: Vec<String>,
    pub window_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DecodeQuality {
    pub archived_raw_count: u64,
    pub successfully_decoded_raw_count: u64,
    pub schema_incompatible_raw_count: u64,
    pub archived_not_decoded_count: u64,
    pub decode_success_fraction: f64,
    pub decode_failure_fraction: f64,
    pub quality_gate_failures: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CorpusManifest {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub operation: String,
    pub source: String,
    pub requested_mode: Option<HistoryMode>,
    pub effective_mode: HistoryMode,
    pub fallback_reason: Option<String>,
    pub programs: Vec<String>,
    pub raw_count: u64,
    pub decoded_count: u64,
    pub quarantine_count: u64,
    pub canonical_event_count: u64,
    pub true_launch_created_event_count: u64,
    pub true_launch_created_unique_mints: u64,
    pub provisional_first_trade_launch_count: u64,
    pub unique_launch_observations: u64,
    pub unique_mints: u64,
    pub events_by_protocol: BTreeMap<String, u64>,
    pub min_slot: Option<String>,
    pub max_slot: Option<String>,
    pub decoder_version: String,
    pub hashes: CorpusHashes,
    pub credit_accounting: CreditAccounting,
    pub calendar_coverage: CalendarCoverage,
    pub provider_failures: Vec<FailureAggregate>,
    pub decode_failures: Vec<FailureAggregate>,
    pub provider_failure_count: u64,
    pub decode_failure_count: u64,
    pub recovered_transient_errors: Vec<FailureAggregate>,
    pub recovered_transient_error_count: u64,
    pub decode_quality: DecodeQuality,
    pub failure_count: u64,
    pub failures_truncated: bool,
    pub completeness: Completeness,
    pub program_window_checkpoints: BTreeMap<String, StreamCheckpoint>,
    pub canonical_events_uri: String,
    pub raw_index_uri: String,
}

pub fn ensure_quality(manifest: &CorpusManifest) -> anyhow::Result<()> {
    if manifest.completeness.status == "failed_quality_gate" {
        bail!(
            "historical corpus failed quality gates: {}",
            manifest.decode_quality.quality_gate_failures.join("; ")
        );
    }
    if manifest.canonical_event_count == 0 {
        bail!("historical corpus has zero canonical events");
    }
    if manifest.provider_failure_count > 0 {
        bail!(
            "historical corpus has {} unresolved provider/fetch failures",
            manifest.provider_failure_count
        );
    }
    Ok(())
}

#[derive(Clone)]
struct RateLimiter {
    interval: Duration,
    next: Arc<Mutex<tokio::time::Instant>>,
}

impl RateLimiter {
    fn new(requests_per_second: f64) -> Self {
        Self {
            interval: Duration::from_secs_f64(1.0 / requests_per_second),
            next: Arc::new(Mutex::new(tokio::time::Instant::now())),
        }
    }

    async fn wait(&self) {
        let mut next = self.next.lock().await;
        let now = tokio::time::Instant::now();
        if *next > now {
            tokio::time::sleep_until(*next).await;
        }
        *next = tokio::time::Instant::now() + self.interval;
    }

    async fn defer_for(&self, duration: Duration) {
        let mut next = self.next.lock().await;
        let deferred = tokio::time::Instant::now() + duration;
        if deferred > *next {
            *next = deferred;
        }
    }
}

#[derive(Clone, Debug)]
struct RpcFailure {
    message: String,
    plan_denial: bool,
    transient: bool,
    attempts: u32,
    transient_attempts: Vec<TransientAttempt>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TransientAttempt {
    error: String,
    retry_after_used: bool,
}

struct RpcSuccess {
    value: Value,
    transient_attempts: Vec<TransientAttempt>,
}

#[derive(Clone)]
struct RpcClient {
    http: reqwest::Client,
    rpc_url: String,
    secret: Option<String>,
    limiter: RateLimiter,
    retries: u32,
    backoff_ms: u64,
}

impl RpcClient {
    fn new(
        rpc_url: String,
        secret: Option<String>,
        config: &HistoryConfig,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(config.request_timeout_secs))
                .build()?,
            rpc_url,
            secret,
            limiter: RateLimiter::new(config.rate_limit_per_second),
            retries: config.retries,
            backoff_ms: config.backoff_ms,
        })
    }

    fn redact(&self, text: impl Into<String>) -> String {
        let mut text = text.into();
        if let Some(secret) = self.secret.as_deref().filter(|s| !s.is_empty()) {
            text = text.replace(secret, "[REDACTED]");
        }
        if let Some(query) = self.rpc_url.split_once('?').map(|(_, query)| query) {
            if !query.is_empty() {
                text = text.replace(query, "[REDACTED_QUERY]");
            }
        }
        text
    }

    async fn call(&self, method: &str, params: Value) -> Result<RpcSuccess, RpcFailure> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let mut last = String::new();
        let mut was_transient = false;
        let mut transient_attempts = Vec::new();
        for attempt in 0..=self.retries {
            self.limiter.wait().await;
            match self.http.post(&self.rpc_url).json(&body).send().await {
                Ok(response) => {
                    let status = response.status();
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(parse_retry_after);
                    let bytes = match response.bytes().await {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            last = format!("read response: {error}");
                            if attempt < self.retries {
                                self.defer_backoff(method, attempt).await;
                                continue;
                            }
                            break;
                        }
                    };
                    if !status.is_success() {
                        last = format!(
                            "HTTP {}: {}",
                            status.as_u16(),
                            String::from_utf8_lossy(&bytes)
                        );
                        let denial =
                            matches!(status.as_u16(), 402 | 403) || is_plan_denial_text(&last);
                        if denial {
                            return Err(RpcFailure {
                                message: self.redact(last),
                                plan_denial: true,
                                transient: false,
                                attempts: attempt + 1,
                                transient_attempts,
                            });
                        }
                        was_transient = status.as_u16() == 429 || status.is_server_error();
                        if was_transient {
                            transient_attempts.push(TransientAttempt {
                                error: self.redact(last.clone()),
                                retry_after_used: retry_after.is_some(),
                            });
                            if attempt < self.retries {
                                if let Some(delay) = retry_after {
                                    self.limiter.defer_for(delay).await;
                                } else {
                                    self.defer_backoff(method, attempt).await;
                                }
                                continue;
                            }
                        }
                        break;
                    }
                    let value: Value = match serde_json::from_slice(&bytes) {
                        Ok(value) => value,
                        Err(error) => {
                            last = format!("invalid JSON-RPC response: {error}");
                            if attempt < self.retries {
                                self.defer_backoff(method, attempt).await;
                                continue;
                            }
                            break;
                        }
                    };
                    if let Some(error) = value.get("error") {
                        last = format!("JSON-RPC error: {error}");
                        let code = error.get("code").and_then(Value::as_i64);
                        let denial = code == Some(-32601) || is_plan_denial_text(&last);
                        if denial {
                            return Err(RpcFailure {
                                message: self.redact(last),
                                plan_denial: true,
                                transient: false,
                                attempts: attempt + 1,
                                transient_attempts,
                            });
                        }
                        if attempt < self.retries {
                            self.defer_backoff(method, attempt).await;
                            continue;
                        }
                        break;
                    }
                    return Ok(RpcSuccess {
                        value: value.get("result").cloned().unwrap_or(Value::Null),
                        transient_attempts,
                    });
                }
                Err(error) => {
                    last = format!("HTTP transport: {error}");
                    was_transient = true;
                    transient_attempts.push(TransientAttempt {
                        error: self.redact(last.clone()),
                        retry_after_used: false,
                    });
                    if attempt < self.retries {
                        self.defer_backoff(method, attempt).await;
                        continue;
                    }
                    break;
                }
            }
        }
        Err(RpcFailure {
            message: self.redact(last),
            plan_denial: false,
            transient: was_transient,
            attempts: self.retries + 1,
            transient_attempts,
        })
    }

    async fn defer_backoff(&self, method: &str, attempt: u32) {
        let factor = 1u64.checked_shl(attempt.min(10)).unwrap_or(1_024);
        let base = self.backoff_ms.saturating_mul(factor).min(60_000);
        let jitter_bound = (base / 4).min(1_000).max(1);
        let seed = method.bytes().fold(attempt as u64 + 1, |hash, byte| {
            hash.wrapping_mul(1099511628211).wrapping_add(byte as u64)
        });
        let delay = base.saturating_add(seed % jitter_bound).min(60_000);
        self.limiter.defer_for(Duration::from_millis(delay)).await;
    }

    async fn full_history_page(
        &self,
        program_id: &str,
        window: &TimeWindow,
        pagination_token: Option<&str>,
        limit: usize,
    ) -> Result<FullPage, RpcFailure> {
        let mut options = json!({
            "transactionDetails": "full",
            "sortOrder": "asc",
            "limit": limit,
            "commitment": "finalized",
            "encoding": "json",
            "maxSupportedTransactionVersion": 0,
            "filters": {
                "status": "succeeded",
                "blockTime": {
                    "gte": window.gte.timestamp(),
                    "lte": window.lte.timestamp()
                }
            }
        });
        if let Some(token) = pagination_token {
            options["paginationToken"] = json!(token);
        }
        let result = self
            .call("getTransactionsForAddress", json!([program_id, options]))
            .await?;
        let mut page = parse_full_page(result.value, limit).map_err(|message| RpcFailure {
            message,
            plan_denial: false,
            transient: false,
            attempts: 1,
            transient_attempts: result.transient_attempts.clone(),
        })?;
        page.transient_attempts = result.transient_attempts;
        Ok(page)
    }

    async fn signatures(
        &self,
        program_id: &str,
        before: Option<&str>,
        config: &HistoryConfig,
    ) -> Result<(Vec<SignatureInfo>, Vec<TransientAttempt>), RpcFailure> {
        let mut options = json!({"limit":config.page_size,"commitment":"finalized"});
        if let Some(before) = before {
            options["before"] = json!(before);
        }
        if let Some(until) = config.until.as_deref() {
            options["until"] = json!(until);
        }
        let result = self
            .call("getSignaturesForAddress", json!([program_id, options]))
            .await?;
        let page = serde_json::from_value(result.value).map_err(|error| RpcFailure {
            message: format!("invalid getSignaturesForAddress result: {error}"),
            plan_denial: false,
            transient: false,
            attempts: 1,
            transient_attempts: result.transient_attempts.clone(),
        })?;
        Ok((page, result.transient_attempts))
    }

    async fn transaction(
        &self,
        signature: &str,
        config: &HistoryConfig,
    ) -> Result<(Option<RawTransaction>, Vec<TransientAttempt>), RpcFailure> {
        let result = self
            .call(
                "getTransaction",
                json!([
                    signature,
                    {
                        "encoding":"json",
                        "maxSupportedTransactionVersion":config.max_supported_transaction_version,
                        "commitment":"finalized"
                    }
                ]),
            )
            .await?;
        Ok((
            (!result.value.is_null()).then(|| parse_transaction(signature, &result.value)),
            result.transient_attempts,
        ))
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = DateTime::parse_from_rfc2822(value.trim())
        .ok()?
        .with_timezone(&Utc);
    Some(
        (at - Utc::now())
            .to_std()
            .unwrap_or_else(|_| Duration::ZERO),
    )
}

fn is_plan_denial_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "method not found",
        "method not available",
        "not supported on your plan",
        "upgrade your plan",
        "plan does not include",
        "payment required",
        "forbidden",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

struct FullPage {
    records: Vec<RawTransaction>,
    pagination_token: Option<String>,
    transient_attempts: Vec<TransientAttempt>,
}

fn parse_full_page(result: Value, requested_limit: usize) -> Result<FullPage, String> {
    let object = result
        .as_object()
        .ok_or_else(|| "getTransactionsForAddress result must be an object".to_string())?;
    let data = object
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "getTransactionsForAddress result.data must be an array".to_string())?;
    if data.len() > requested_limit {
        return Err(format!(
            "getTransactionsForAddress returned {} records for limit {requested_limit}",
            data.len()
        ));
    }
    let pagination_token = match object.get("paginationToken") {
        Some(Value::String(token)) if !token.is_empty() => Some(token.clone()),
        Some(Value::Null) => None,
        Some(_) => {
            return Err(
                "getTransactionsForAddress paginationToken must be string or null".to_string(),
            )
        }
        None => return Err("getTransactionsForAddress paginationToken is missing".to_string()),
    };
    let mut records = Vec::with_capacity(data.len());
    for (index, row) in data.iter().enumerate() {
        let row = row
            .as_object()
            .ok_or_else(|| format!("full record {index} must be an object"))?;
        for required in ["slot", "blockTime", "transaction", "meta"] {
            if !row.contains_key(required) {
                return Err(format!("full record {index} missing {required}"));
            }
        }
        if row.get("slot").and_then(Value::as_u64).is_none()
            || !row.get("transaction").is_some_and(Value::is_object)
            || !row.get("meta").is_some_and(Value::is_object)
            || !(row.get("blockTime").is_some_and(Value::is_null)
                || row.get("blockTime").and_then(Value::as_i64).is_some())
        {
            return Err(format!("full record {index} has invalid field types"));
        }
        if !row
            .get("meta")
            .and_then(|meta| meta.get("err"))
            .is_some_and(Value::is_null)
        {
            return Err(format!(
                "full record {index} is not succeeded despite status filter"
            ));
        }
        let signature = row
            .get("transaction")
            .and_then(|tx| tx.get("signatures"))
            .and_then(Value::as_array)
            .and_then(|signatures| signatures.first())
            .and_then(Value::as_str)
            .ok_or_else(|| format!("full record {index} missing transaction.signatures[0]"))?;
        records.push(parse_transaction(signature, &Value::Object(row.clone())));
    }
    records.sort_by(|a, b| {
        (a.slot.0, a.transaction_index, &a.signature).cmp(&(
            b.slot.0,
            b.transaction_index,
            &b.signature,
        ))
    });
    Ok(FullPage {
        records,
        pagination_token,
        transient_attempts: Vec::new(),
    })
}

#[derive(Clone)]
struct CorpusWriter {
    root: PathBuf,
    canonical_root: PathBuf,
    decode_state_root: PathBuf,
}

struct IngestOutcome {
    raw_added: bool,
    events: Vec<ChainEvent>,
}

impl CorpusWriter {
    fn new(config: &HistoryConfig) -> Self {
        let root = PathBuf::from(&config.output_dir);
        Self {
            canonical_root: root.join("canonical"),
            decode_state_root: root.join("decode-state"),
            root,
        }
    }

    fn for_redecode(root: &Path, staging: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            canonical_root: staging.join("canonical"),
            decode_state_root: staging.join("decode-state"),
        }
    }

    async fn ingest(&self, tx: &RawTransaction) -> anyhow::Result<IngestOutcome> {
        let raw_hash = payload_sha256(&serde_json::to_vec(tx)?);
        let day = tx
            .block_time
            .map(|time| time.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".into());
        let relative = format!("raw/date={day}/{}.json", tx.signature);
        let archive_path = self.root.join(&relative);
        let raw_added = !archive_path.exists();
        let archive = json!({
            "source":"history-backfill",
            "signature":tx.signature,
            "slot":tx.slot,
            "block_time":tx.block_time,
            "received_at":Utc::now(),
            "commitment":"finalized",
            "decoder_version":DECODER_VERSION,
            "payload_sha256":raw_hash,
            "transaction":tx,
        });
        write_raw_idempotent(
            &archive_path,
            &serde_json::to_vec_pretty(&archive)?,
            &raw_hash,
        )
        .await?;

        let (events, _decode_state) = self.decode_transaction(tx, &relative, &raw_hash).await?;
        Ok(IngestOutcome { raw_added, events })
    }

    async fn decode_transaction(
        &self,
        tx: &RawTransaction,
        raw_partition_uri: &str,
        raw_hash: &str,
    ) -> anyhow::Result<(Vec<ChainEvent>, DecodeState)> {
        let pump = PumpBondingCurveDecoder::default();
        let pumpswap = PumpSwapDecoder::default();
        let mut emitted = Vec::new();
        let mut issues = Vec::new();
        for (name, decoder) in [
            ("pump", &pump as &dyn LaunchpadDecoder),
            ("pumpswap", &pumpswap as &dyn LaunchpadDecoder),
        ] {
            match decoder.decode(tx) {
                Ok(events) => {
                    for event in events {
                        let event = canonicalize_event(event, tx, raw_partition_uri, raw_hash);
                        write_idempotent(
                            &self
                                .canonical_root
                                .join("by-event-id")
                                .join(format!("{}.json", event.event_id)),
                            &serde_json::to_vec(&event)?,
                        )
                        .await?;
                        emitted.push(event);
                    }
                }
                Err(error) => issues.push(DecodeIssue {
                    program_id: decoder.program_ids().first().cloned().unwrap_or_default(),
                    method: format!("decode:{name}"),
                    error: error.to_string(),
                }),
            }
        }
        let state = DecodeState {
            signature: tx.signature.clone(),
            raw_payload_sha256: raw_hash.into(),
            decoder_version: DECODER_VERSION.into(),
            status: if issues.is_empty() {
                DecodeStatus::Success
            } else {
                DecodeStatus::SchemaIncompatible
            },
            canonical_event_count: emitted.len() as u64,
            quarantine_count: emitted
                .iter()
                .filter(|event| matches!(event.payload, ChainEventPayload::DecodeQuarantined(_)))
                .count() as u64,
            event_ids: emitted.iter().map(|event| event.event_id.clone()).collect(),
            issues,
        };
        atomic_write(
            &self
                .decode_state_root
                .join(format!("{}.json", tx.signature)),
            &serde_json::to_vec_pretty(&state)?,
        )
        .await?;
        Ok((emitted, state))
    }
}

async fn reprocess_archived(
    root: &Path,
    writer: &CorpusWriter,
    force: bool,
) -> anyhow::Result<u64> {
    let mut processed = 0u64;
    for path in files_below(&root.join("raw"), "json")? {
        let archive: Value = serde_json::from_slice(&std::fs::read(&path)?)
            .with_context(|| format!("parse raw archive {}", path.display()))?;
        let signature = archive
            .get("signature")
            .and_then(Value::as_str)
            .or_else(|| path.file_stem().and_then(|name| name.to_str()))
            .unwrap_or("unknown")
            .to_string();
        let raw_hash = archive
            .get("payload_sha256")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let marker_path = writer.decode_state_root.join(format!("{signature}.json"));
        let existing = read_decode_state(&marker_path);
        let current = existing.as_ref().is_some_and(|state| {
            state.decoder_version == DECODER_VERSION && state.raw_payload_sha256 == raw_hash
        });
        if !force && current {
            continue;
        }
        if !force {
            if let Some(state) = existing {
                for event_id in state.event_ids {
                    let path = writer
                        .canonical_root
                        .join("by-event-id")
                        .join(format!("{event_id}.json"));
                    if path.exists() {
                        tokio::fs::remove_file(path).await?;
                    }
                }
            }
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        match archive.get("transaction").cloned() {
            Some(transaction) => match serde_json::from_value::<RawTransaction>(transaction) {
                Ok(tx) => {
                    writer.decode_transaction(&tx, &relative, &raw_hash).await?;
                }
                Err(error) => {
                    write_decode_schema_failure(
                        &marker_path,
                        &signature,
                        &raw_hash,
                        "archive_schema",
                        error.to_string(),
                    )
                    .await?;
                }
            },
            None => {
                write_decode_schema_failure(
                    &marker_path,
                    &signature,
                    &raw_hash,
                    "archive_schema",
                    "raw archive is missing transaction".into(),
                )
                .await?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

fn read_decode_state(path: &Path) -> Option<DecodeState> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

async fn write_decode_schema_failure(
    path: &Path,
    signature: &str,
    raw_hash: &str,
    method: &str,
    error: String,
) -> anyhow::Result<()> {
    let state = DecodeState {
        signature: signature.into(),
        raw_payload_sha256: raw_hash.into(),
        decoder_version: DECODER_VERSION.into(),
        status: DecodeStatus::SchemaIncompatible,
        canonical_event_count: 0,
        quarantine_count: 0,
        event_ids: Vec::new(),
        issues: vec![DecodeIssue {
            program_id: String::new(),
            method: method.into(),
            error,
        }],
    };
    atomic_write(path, &serde_json::to_vec_pretty(&state)?).await
}

async fn write_canonical_jsonl(canonical_root: &Path) -> anyhow::Result<Vec<u8>> {
    let mut events = Vec::new();
    for path in files_below(&canonical_root.join("by-event-id"), "json")? {
        events.push(serde_json::from_slice::<ChainEvent>(&std::fs::read(path)?)?);
    }
    events.sort_by(|a, b| {
        (a.slot.0, a.tx_index, &a.signature, a.event_index).cmp(&(
            b.slot.0,
            b.tx_index,
            &b.signature,
            b.event_index,
        ))
    });
    let rows = events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = jsonl_bytes(&rows)?;
    atomic_write(&canonical_root.join("events.jsonl"), &bytes).await?;
    Ok(bytes)
}

fn canonicalize_event(
    mut event: ChainEvent,
    tx: &RawTransaction,
    raw_partition_uri: &str,
    raw_hash: &str,
) -> ChainEvent {
    let stable_time = tx
        .block_time
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("Unix epoch"));
    event.observed_at = stable_time;
    event.computed_at = stable_time;
    event.commitment = Commitment::Finalized;
    event.canonicality = Canonicality::Canonical;
    event.provenance.source = "history-backfill".into();
    event.provenance.source_record_id = tx.signature.clone();
    event.provenance.payload_sha256 = raw_hash.into();
    event.provenance.raw_partition_uri = raw_partition_uri.into();
    if let ChainEventPayload::LaunchCreated(created) = &mut event.payload {
        created.launch_id = deterministic_uuid(&event.event_id);
    }
    event
}

fn deterministic_uuid(event_id: &str) -> String {
    let mut hex = event_id.chars().take(32).collect::<String>();
    while hex.len() < 32 {
        hex.push('0');
    }
    hex.replace_range(12..13, "5");
    let variant = u8::from_str_radix(&hex[16..17], 16).unwrap_or(0);
    hex.replace_range(16..17, &format!("{:x}", (variant & 3) | 8));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[derive(Default)]
struct CorpusTracker {
    raw_signatures: BTreeSet<String>,
    event_ids: BTreeSet<String>,
    created_event_ids: BTreeSet<String>,
    created_mints: BTreeSet<String>,
    trade_mints: BTreeSet<String>,
    unique_mints: BTreeSet<String>,
    events_by_protocol: BTreeMap<String, u64>,
}

impl CorpusTracker {
    fn load(root: &Path) -> anyhow::Result<Self> {
        let mut tracker = Self::default();
        for path in files_below(&root.join("raw"), "json")? {
            let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
            if let Some(signature) = value.get("signature").and_then(Value::as_str) {
                tracker.raw_signatures.insert(signature.into());
            }
        }
        for path in files_below(&root.join("canonical/by-event-id"), "json")? {
            tracker.observe(&serde_json::from_slice(&std::fs::read(path)?)?);
        }
        Ok(tracker)
    }

    fn observe(&mut self, event: &ChainEvent) {
        if !self.event_ids.insert(event.event_id.clone()) {
            return;
        }
        *self
            .events_by_protocol
            .entry(protocol_name(event.protocol).into())
            .or_default() += 1;
        match &event.payload {
            ChainEventPayload::LaunchCreated(created) => {
                self.created_event_ids.insert(event.event_id.clone());
                self.created_mints.insert(created.mint.to_string());
                self.unique_mints.insert(created.mint.to_string());
            }
            ChainEventPayload::TradeExecuted(trade) => {
                self.trade_mints.insert(trade.mint.to_string());
                self.unique_mints.insert(trade.mint.to_string());
            }
            ChainEventPayload::LaunchMigrated(migrated) => {
                self.unique_mints.insert(migrated.mint.to_string());
            }
            ChainEventPayload::LaunchCompleted(completed) => {
                self.unique_mints.insert(completed.mint.to_string());
            }
            ChainEventPayload::LiquidityChanged(liquidity) => {
                self.unique_mints.insert(liquidity.mint.to_string());
            }
            _ => {}
        }
    }

    fn launch_observations(&self) -> usize {
        self.created_mints.union(&self.trade_mints).count()
    }

    fn provisional_first_trades(&self) -> usize {
        self.trade_mints.difference(&self.created_mints).count()
    }
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::PumpCurve => "pump_curve",
        Protocol::PumpSwap => "pump_swap",
    }
}

fn existing_decode_preflight_fails(
    root: &Path,
    tracker: &CorpusTracker,
    config: &HistoryConfig,
) -> bool {
    let raw_count = tracker.raw_signatures.len();
    if raw_count == 0 {
        return false;
    }
    let mut successes = 0usize;
    let mut failures = 0usize;
    for signature in &tracker.raw_signatures {
        let state = read_decode_state(&root.join("decode-state").join(format!("{signature}.json")));
        match state {
            Some(state)
                if state.decoder_version == DECODER_VERSION
                    && state.status == DecodeStatus::Success =>
            {
                successes += 1;
            }
            _ => failures += 1,
        }
    }
    let success_fraction = successes as f64 / raw_count as f64;
    let failure_fraction = failures as f64 / raw_count as f64;
    tracker.event_ids.is_empty()
        || success_fraction < config.minimum_decode_success_fraction
        || failure_fraction > config.maximum_decode_failure_fraction
}

pub async fn run(config: HistoryConfig, config_sha256: String) -> anyhow::Result<CorpusManifest> {
    let source = SourceConfig::from_env();
    run_with_rpc(config, config_sha256, source.rpc_url, source.helius_api_key).await
}

pub async fn redecode(
    config: HistoryConfig,
    config_sha256: String,
) -> anyhow::Result<CorpusManifest> {
    config.validate()?;
    let root = PathBuf::from(&config.output_dir);
    if !root.join("raw").is_dir() {
        bail!("raw archive not found under {}", config.output_dir);
    }
    let checkpoint_path = root.join("checkpoint.json");
    let initial_mode = config.history_mode.unwrap_or(HistoryMode::HeliusFull);
    let mut checkpoint =
        load_checkpoint(&checkpoint_path, &config_sha256, initial_mode, &config).await?;
    checkpoint.decoder_version = DECODER_VERSION.into();
    checkpoint.decode_failures.clear();

    let staging = root.join(format!(".redecode-stage-{}", std::process::id()));
    if staging.exists() {
        tokio::fs::remove_dir_all(&staging).await?;
    }
    tokio::fs::create_dir_all(&staging).await?;
    let writer = CorpusWriter::for_redecode(&root, &staging);
    reprocess_archived(&root, &writer, true).await?;
    write_canonical_jsonl(&writer.canonical_root).await?;
    swap_derived_directory(&root, &staging, "canonical").await?;
    swap_derived_directory(&root, &staging, "decode-state").await?;
    if staging.exists() {
        if let Err(error) = tokio::fs::remove_dir_all(&staging).await {
            warn!(%error, path = %staging.display(), "redecode staging cleanup deferred");
        }
    }
    save_checkpoint(&checkpoint_path, &checkpoint).await?;
    let tracker = CorpusTracker::load(&root)?;
    build_manifest(
        &config,
        &config_sha256,
        &checkpoint,
        &checkpoint_path,
        &tracker,
        "redecode",
        false,
    )
    .await
}

async fn swap_derived_directory(root: &Path, staging: &Path, name: &str) -> anyhow::Result<()> {
    let current = root.join(name);
    let replacement = staging.join(name);
    let backup = root.join(format!(".{name}.backup-{}", std::process::id()));
    if backup.exists() {
        tokio::fs::remove_dir_all(&backup).await?;
    }
    if current.exists() {
        tokio::fs::rename(&current, &backup).await?;
    }
    if let Err(error) = tokio::fs::rename(&replacement, &current).await {
        if backup.exists() {
            let _ = tokio::fs::rename(&backup, &current).await;
        }
        return Err(error.into());
    }
    if backup.exists() {
        if let Err(error) = tokio::fs::remove_dir_all(&backup).await {
            warn!(%error, path = %backup.display(), "derived backup cleanup deferred");
        }
    }
    Ok(())
}

async fn run_with_rpc(
    config: HistoryConfig,
    config_sha256: String,
    rpc_url: String,
    secret: Option<String>,
) -> anyhow::Result<CorpusManifest> {
    config.validate()?;
    let initial_mode = match config.history_mode {
        Some(HistoryMode::StandardRpc) => HistoryMode::StandardRpc,
        Some(HistoryMode::HeliusFull) if secret.is_some() => HistoryMode::HeliusFull,
        Some(HistoryMode::HeliusFull) => bail!("helius_full requires PUMPHOUND_HELIUS_API_KEY"),
        None if secret.is_some() => HistoryMode::HeliusFull,
        None => bail!(
            "no Helius key available; set history_mode to standard_rpc explicitly to fall back"
        ),
    };
    let root = PathBuf::from(&config.output_dir);
    tokio::fs::create_dir_all(&root).await?;
    let checkpoint_path = root.join("checkpoint.json");
    let mut checkpoint =
        load_checkpoint(&checkpoint_path, &config_sha256, initial_mode, &config).await?;
    ensure_streams(&mut checkpoint, &config);
    let writer = CorpusWriter::new(&config);
    reprocess_archived(&root, &writer, false).await?;
    let mut tracker = CorpusTracker::load(&root)?;
    save_checkpoint(&checkpoint_path, &checkpoint).await?;
    if existing_decode_preflight_fails(&root, &tracker, &config) {
        return build_manifest(
            &config,
            &config_sha256,
            &checkpoint,
            &checkpoint_path,
            &tracker,
            "resume_decode_preflight",
            true,
        )
        .await;
    }
    let rpc = RpcClient::new(rpc_url, secret, &config)?;
    let mut pending_attempted = BTreeSet::new();

    'runner: loop {
        checkpoint.global_stop_reason = stop_reason(&config, &checkpoint, &tracker);
        if checkpoint.global_stop_reason.is_some() || all_streams_complete(&checkpoint) {
            break;
        }
        let mut progressed = false;
        let keys = ordered_stream_keys(&checkpoint, &config);
        for key in keys {
            if checkpoint.streams.get(&key).is_none_or(|stream| {
                stream.complete
                    && (stream.pending_transactions.is_empty() || pending_attempted.contains(&key))
            }) {
                continue;
            }
            checkpoint.global_stop_reason = stop_reason(&config, &checkpoint, &tracker);
            if checkpoint.global_stop_reason.is_some() {
                break 'runner;
            }
            let mut state = checkpoint.streams.remove(&key).expect("stream exists");
            let window = config
                .windows
                .iter()
                .find(|window| window.id == state.window_id)
                .expect("validated window");
            let mut stop_after_stream = false;
            match checkpoint.effective_mode {
                HistoryMode::HeliusFull => {
                    let remaining = config
                        .max_raw_transactions
                        .saturating_sub(tracker.raw_signatures.len());
                    let limit = config.page_size.min(remaining.max(1));
                    checkpoint.total_requests += 1;
                    let page = rpc
                        .full_history_page(
                            &state.program_id,
                            window,
                            state.pagination_token.as_deref(),
                            limit,
                        )
                        .await;
                    match page {
                        Ok(page) => {
                            recover_prior_transient(&mut state);
                            record_recovered_attempts(
                                &mut state,
                                "getTransactionsForAddress",
                                page.transient_attempts,
                            );
                            checkpoint.helius_full_successful_requests += 1;
                            checkpoint.helius_full_records_returned += page.records.len() as u64;
                            checkpoint.helius_metered_credits_estimate +=
                                metered_credits(page.records.len());
                            state.pages += 1;
                            state.records_returned += page.records.len() as u64;
                            state.last_error = None;
                            state.stop_reason = None;
                            for tx in page.records {
                                if stop_reason(&config, &checkpoint, &tracker).is_some() {
                                    break;
                                }
                                ingest_and_track(
                                    &writer,
                                    &mut tracker,
                                    &mut checkpoint,
                                    &state,
                                    tx,
                                )
                                .await?;
                            }
                            state.pagination_token = page.pagination_token;
                            if state.pagination_token.is_none() {
                                state.complete = true;
                                state.stop_reason = Some("window_exhausted".into());
                            }
                            progressed = true;
                        }
                        Err(error)
                            if error.plan_denial && config.allow_standard_fallback_on_denial =>
                        {
                            checkpoint.fallback_reason = Some(format!(
                                "getTransactionsForAddress denied: {}",
                                error.message
                            ));
                            checkpoint.effective_mode = HistoryMode::StandardRpc;
                            for stream in checkpoint.streams.values_mut() {
                                if !stream.complete {
                                    stream.pagination_token = None;
                                    stream.before = config.before.clone();
                                    stream.last_error = None;
                                }
                            }
                            state.pagination_token = None;
                            state.before = config.before.clone();
                            state.last_error = None;
                            progressed = true;
                        }
                        Err(error) => {
                            let mut failure = failure(
                                &state,
                                "getTransactionsForAddress",
                                error.message,
                                error.attempts,
                                !error.plan_denial,
                                error.transient,
                            );
                            failure.transient_attempt_history = error.transient_attempts;
                            state.last_error = Some(failure);
                            state.stop_reason = Some(if error.plan_denial {
                                "plan_denial_no_fallback".into()
                            } else if error.transient {
                                "transient_retries_exhausted".into()
                            } else {
                                "rpc_error".into()
                            });
                            if error.plan_denial || !error.transient {
                                checkpoint.global_stop_reason = state.stop_reason.clone();
                                stop_after_stream = true;
                            } else {
                                progressed = true;
                            }
                        }
                    }
                }
                HistoryMode::StandardRpc => {
                    let retry_pending_on_entry = pending_attempted.insert(key.clone());
                    process_standard_page(
                        &rpc,
                        &writer,
                        &config,
                        window,
                        &mut checkpoint,
                        &mut state,
                        &mut tracker,
                        retry_pending_on_entry,
                    )
                    .await?;
                    progressed = true;
                }
            }
            checkpoint.streams.insert(key, state);
            save_checkpoint(&checkpoint_path, &checkpoint).await?;
            if stop_after_stream {
                break 'runner;
            }
            if checkpoint.effective_mode == HistoryMode::StandardRpc
                && checkpoint.fallback_reason.is_some()
            {
                break;
            }
            if config.window_strategy == WindowStrategy::Sequential {
                break;
            }
        }
        if !progressed {
            checkpoint.global_stop_reason = Some("no_progress".into());
            break;
        }
    }
    save_checkpoint(&checkpoint_path, &checkpoint).await?;
    build_manifest(
        &config,
        &config_sha256,
        &checkpoint,
        &checkpoint_path,
        &tracker,
        "provider_backfill",
        true,
    )
    .await
}

async fn process_standard_page(
    rpc: &RpcClient,
    writer: &CorpusWriter,
    config: &HistoryConfig,
    window: &TimeWindow,
    checkpoint: &mut HistoryCheckpoint,
    state: &mut StreamCheckpoint,
    tracker: &mut CorpusTracker,
    retry_pending_on_entry: bool,
) -> anyhow::Result<()> {
    if retry_pending_on_entry {
        retry_pending(rpc, writer, config, checkpoint, state, tracker).await?;
    }
    if state.complete {
        return Ok(());
    }
    if stop_reason(config, checkpoint, tracker).is_some() {
        return Ok(());
    }
    checkpoint.total_requests += 1;
    let (page, transient_attempts) = match rpc
        .signatures(&state.program_id, state.before.as_deref(), config)
        .await
    {
        Ok(page) => page,
        Err(error) => {
            let mut record = failure(
                state,
                "getSignaturesForAddress",
                error.message,
                error.attempts,
                true,
                error.transient,
            );
            record.transient_attempt_history = error.transient_attempts;
            state.last_error = Some(record);
            return Ok(());
        }
    };
    recover_prior_transient(state);
    record_recovered_attempts(state, "getSignaturesForAddress", transient_attempts);
    state.pages += 1;
    state.records_returned += page.len() as u64;
    state.last_error = None;
    state.stop_reason = None;
    if page.is_empty() {
        state.complete = true;
        state.stop_reason = Some("window_exhausted".into());
        return Ok(());
    }
    let short = page.len() < config.page_size;
    state.before = page.last().map(|info| info.signature.clone());
    let mut reached_start = false;
    let mut selected = Vec::new();
    for info in page {
        if config.max_slot.is_some_and(|slot| info.slot > slot)
            || info
                .block_time
                .is_some_and(|time| time > window.lte.timestamp())
        {
            continue;
        }
        if config.min_slot.is_some_and(|slot| info.slot < slot)
            || info
                .block_time
                .is_some_and(|time| time < window.gte.timestamp())
        {
            reached_start = true;
            break;
        }
        selected.push(info);
    }
    selected.reverse();
    let available_requests = config
        .max_requests
        .saturating_sub(checkpoint.total_requests) as usize;
    selected.truncate(available_requests);
    checkpoint.total_requests += selected.len() as u64;
    let results = stream::iter(selected)
        .map(|info| {
            let rpc = rpc.clone();
            async move {
                let result = rpc.transaction(&info.signature, config).await;
                (info, result)
            }
        })
        .buffer_unordered(config.concurrency)
        .collect::<Vec<_>>()
        .await;
    for (info, result) in results {
        match result {
            Ok((Some(tx), transient_attempts)) => {
                record_recovered_attempts(state, "getTransaction", transient_attempts);
                if let Some(pending) = state.pending_transactions.remove(&info.signature) {
                    if pending.failure.transient {
                        record_recovered_failure(state, pending.failure);
                    }
                }
                ingest_and_track(writer, tracker, checkpoint, state, tx).await?;
            }
            Ok((None, transient_attempts)) => {
                record_recovered_attempts(state, "getTransaction", transient_attempts);
                add_pending(
                    state,
                    info,
                    "null getTransaction result".into(),
                    1,
                    false,
                    Vec::new(),
                );
            }
            Err(error) => add_pending(
                state,
                info,
                error.message,
                error.attempts,
                error.transient,
                error.transient_attempts,
            ),
        }
    }
    if reached_start || short {
        state.complete = true;
        state.stop_reason = Some("window_exhausted".into());
    }
    Ok(())
}

async fn retry_pending(
    rpc: &RpcClient,
    writer: &CorpusWriter,
    config: &HistoryConfig,
    checkpoint: &mut HistoryCheckpoint,
    state: &mut StreamCheckpoint,
    tracker: &mut CorpusTracker,
) -> anyhow::Result<()> {
    let pending = state
        .pending_transactions
        .values()
        .map(|pending| pending.info.clone())
        .collect::<Vec<_>>();
    for info in pending {
        if checkpoint.total_requests >= config.max_requests {
            break;
        }
        checkpoint.total_requests += 1;
        match rpc.transaction(&info.signature, config).await {
            Ok((Some(tx), transient_attempts)) => {
                record_recovered_attempts(state, "getTransaction", transient_attempts);
                if let Some(pending) = state.pending_transactions.remove(&info.signature) {
                    if pending.failure.transient {
                        record_recovered_failure(state, pending.failure);
                    }
                }
                ingest_and_track(writer, tracker, checkpoint, state, tx).await?;
            }
            Ok((None, transient_attempts)) => {
                record_recovered_attempts(state, "getTransaction", transient_attempts);
                add_pending(
                    state,
                    info,
                    "null getTransaction result".into(),
                    1,
                    false,
                    Vec::new(),
                );
            }
            Err(error) => add_pending(
                state,
                info,
                error.message,
                error.attempts,
                error.transient,
                error.transient_attempts,
            ),
        }
    }
    Ok(())
}

fn add_pending(
    state: &mut StreamCheckpoint,
    info: SignatureInfo,
    error: String,
    attempts: u32,
    transient: bool,
    transient_attempt_history: Vec<TransientAttempt>,
) {
    let signature = info.signature.clone();
    let mut record = failure(state, "getTransaction", error, attempts, true, transient);
    record.transient_attempt_history = transient_attempt_history;
    record.signature = Some(signature.clone());
    state.pending_transactions.insert(
        signature,
        PendingTransaction {
            failure: record,
            info,
        },
    );
}

fn recover_prior_transient(state: &mut StreamCheckpoint) {
    if state
        .last_error
        .as_ref()
        .is_some_and(|failure| failure.transient)
    {
        let failure = state.last_error.take().expect("checked above");
        record_recovered_failure(state, failure);
    }
}

fn record_recovered_failure(state: &mut StreamCheckpoint, failure: FailureRecord) {
    if failure.transient_attempt_history.is_empty() {
        record_recovered_error(
            state,
            &failure.method,
            failure.error,
            failure.attempts as u64,
        );
    } else {
        record_recovered_attempts(state, &failure.method, failure.transient_attempt_history);
    }
}

fn record_recovered_attempts(
    state: &mut StreamCheckpoint,
    method: &str,
    attempts: Vec<TransientAttempt>,
) {
    for attempt in attempts {
        let error = if attempt.retry_after_used {
            format!("{} [Retry-After honored]", attempt.error)
        } else {
            attempt.error
        };
        record_recovered_error(state, method, error, 1);
    }
}

fn record_recovered_error(state: &mut StreamCheckpoint, method: &str, error: String, count: u64) {
    let key = payload_sha256(format!("{method}\0{error}").as_bytes());
    let recovered = state
        .recovered_transient_errors
        .entry(key)
        .or_insert_with(|| RecoveredTransientError {
            method: method.into(),
            error,
            count: 0,
        });
    recovered.count = recovered.count.saturating_add(count);
}

async fn ingest_and_track(
    writer: &CorpusWriter,
    tracker: &mut CorpusTracker,
    _checkpoint: &mut HistoryCheckpoint,
    _state: &StreamCheckpoint,
    tx: RawTransaction,
) -> anyhow::Result<()> {
    let outcome = writer.ingest(&tx).await?;
    if outcome.raw_added {
        tracker.raw_signatures.insert(tx.signature);
    }
    for event in &outcome.events {
        tracker.observe(event);
    }
    Ok(())
}

fn failure(
    state: &StreamCheckpoint,
    method: &str,
    error: String,
    attempts: u32,
    retryable: bool,
    transient: bool,
) -> FailureRecord {
    FailureRecord {
        program_id: state.program_id.clone(),
        window_id: state.window_id.clone(),
        signature: None,
        method: method.into(),
        error,
        attempts,
        retryable_on_resume: retryable,
        transient,
        transient_attempt_history: Vec::new(),
    }
}

fn metered_credits(records: usize) -> u64 {
    if records == 0 {
        10
    } else {
        (records as u64).div_ceil(100) * 10
    }
}

fn stop_reason(
    config: &HistoryConfig,
    checkpoint: &HistoryCheckpoint,
    tracker: &CorpusTracker,
) -> Option<String> {
    if tracker.launch_observations() >= config.target_launches {
        Some("target_launches_reached".into())
    } else if tracker.raw_signatures.len() >= config.max_raw_transactions {
        Some("max_raw_transactions_reached".into())
    } else if checkpoint.total_requests >= config.max_requests {
        Some("max_requests_reached".into())
    } else {
        None
    }
}

fn stream_key(program_id: &str, window_id: &str) -> String {
    format!("{program_id}:{window_id}")
}

fn ensure_streams(checkpoint: &mut HistoryCheckpoint, config: &HistoryConfig) {
    for window in &config.windows {
        for program in [PUMP_PROGRAM_ID, PUMPSWAP_PROGRAM_ID] {
            checkpoint
                .streams
                .entry(stream_key(program, &window.id))
                .or_insert_with(|| StreamCheckpoint {
                    program_id: program.into(),
                    window_id: window.id.clone(),
                    before: config.before.clone(),
                    ..Default::default()
                });
        }
    }
}

fn ordered_stream_keys(checkpoint: &HistoryCheckpoint, config: &HistoryConfig) -> Vec<String> {
    let mut keys = Vec::new();
    for window in &config.windows {
        for program in [PUMP_PROGRAM_ID, PUMPSWAP_PROGRAM_ID] {
            let key = stream_key(program, &window.id);
            if checkpoint.streams.contains_key(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

fn all_streams_complete(checkpoint: &HistoryCheckpoint) -> bool {
    !checkpoint.streams.is_empty()
        && checkpoint
            .streams
            .values()
            .all(|stream| stream.complete && stream.pending_transactions.is_empty())
}

async fn load_checkpoint(
    path: &Path,
    config_sha256: &str,
    initial_mode: HistoryMode,
    config: &HistoryConfig,
) -> anyhow::Result<HistoryCheckpoint> {
    let semantics_hash = source_semantics_hash(config)?;
    if !path.exists() {
        return Ok(HistoryCheckpoint {
            version: CHECKPOINT_VERSION,
            run_config_hash: config_sha256.into(),
            source_semantics_hash: semantics_hash,
            decoder_version: DECODER_VERSION.into(),
            requested_mode: config.history_mode,
            effective_mode: initial_mode,
            fallback_reason: None,
            total_requests: 0,
            helius_full_successful_requests: 0,
            helius_full_records_returned: 0,
            helius_metered_credits_estimate: 0,
            global_stop_reason: None,
            streams: BTreeMap::new(),
            decode_failures: BTreeMap::new(),
        });
    }
    let mut checkpoint: HistoryCheckpoint = serde_json::from_slice(&tokio::fs::read(path).await?)
        .context("parse history checkpoint")?;
    if checkpoint.version != CHECKPOINT_VERSION {
        bail!("checkpoint version changed; use a new output_dir");
    }
    if checkpoint.source_semantics_hash.is_empty() {
        let known_legacy_config = checkpoint.run_config_hash == config_sha256
            || checkpoint.run_config_hash == legacy_config_sha256(config)?
            || (checkpoint.run_config_hash == PAID_CORPUS_PRE_SEMANTICS_HASH
                && checkpoint_streams_match_config(&checkpoint, config));
        if !known_legacy_config {
            bail!("source semantics changed; use a new output_dir");
        }
        checkpoint.source_semantics_hash = semantics_hash.clone();
    } else if checkpoint.source_semantics_hash != semantics_hash {
        bail!("source semantics changed; use a new output_dir");
    }
    checkpoint.run_config_hash = config_sha256.into();
    for stream in checkpoint.streams.values_mut() {
        if let Some(failure) = stream.last_error.as_mut() {
            failure.transient |= is_transient_error_text(&failure.error);
        }
        for pending in stream.pending_transactions.values_mut() {
            pending.failure.transient |= is_transient_error_text(&pending.failure.error);
        }
    }
    if checkpoint.decoder_version != DECODER_VERSION {
        checkpoint.decoder_version = DECODER_VERSION.into();
    }
    // Decoder errors are derived from immutable raw archives and now live in
    // compact per-signature decode-state records. Keep provider cursors only.
    checkpoint.decode_failures.clear();
    Ok(checkpoint)
}

fn is_transient_error_text(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    if lower.contains("http 429") || lower.contains("too many requests") {
        return true;
    }
    (500..=599).any(|status| lower.contains(&format!("http {status}")))
}

fn checkpoint_streams_match_config(checkpoint: &HistoryCheckpoint, config: &HistoryConfig) -> bool {
    let expected = config
        .windows
        .iter()
        .flat_map(|window| {
            [PUMP_PROGRAM_ID, PUMPSWAP_PROGRAM_ID]
                .into_iter()
                .map(move |program| stream_key(program, &window.id))
        })
        .collect::<BTreeSet<_>>();
    checkpoint.streams.keys().cloned().collect::<BTreeSet<_>>() == expected
}

async fn save_checkpoint(path: &Path, checkpoint: &HistoryCheckpoint) -> anyhow::Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(checkpoint)?).await
}

async fn write_idempotent(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if path.exists() {
        if tokio::fs::read(path).await? != bytes {
            bail!("idempotency collision at {}", path.display());
        }
        return Ok(());
    }
    atomic_write(path, bytes).await
}

async fn write_raw_idempotent(
    path: &Path,
    bytes: &[u8],
    expected_payload_sha256: &str,
) -> anyhow::Result<()> {
    if path.exists() {
        let existing: Value = serde_json::from_slice(&tokio::fs::read(path).await?)?;
        if existing.get("payload_sha256").and_then(Value::as_str) != Some(expected_payload_sha256) {
            bail!("raw archive payload collision at {}", path.display());
        }
        return Ok(());
    }
    atomic_write(path, bytes).await
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    tokio::fs::write(&temporary, bytes).await?;
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}

struct AggregateInput {
    category: &'static str,
    method: String,
    error: String,
    signature: Option<String>,
    program_id: String,
    window_id: String,
    count: u64,
}

fn aggregate_failures(
    inputs: Vec<AggregateInput>,
    max_groups: usize,
    max_examples: usize,
) -> (Vec<FailureAggregate>, u64, bool) {
    struct Builder {
        category: String,
        method: String,
        error: String,
        count: u64,
        signatures: BTreeSet<String>,
        programs: BTreeSet<String>,
        windows: BTreeSet<String>,
    }
    let mut grouped: BTreeMap<(String, String, String), Builder> = BTreeMap::new();
    let mut total = 0u64;
    for input in inputs {
        total = total.saturating_add(input.count);
        let key = (
            input.category.into(),
            input.method.clone(),
            input.error.clone(),
        );
        let group = grouped.entry(key).or_insert_with(|| Builder {
            category: input.category.into(),
            method: input.method,
            error: input.error,
            count: 0,
            signatures: BTreeSet::new(),
            programs: BTreeSet::new(),
            windows: BTreeSet::new(),
        });
        group.count = group.count.saturating_add(input.count);
        if let Some(signature) = input.signature {
            if group.signatures.len() < max_examples {
                group.signatures.insert(signature);
            }
        }
        if !input.program_id.is_empty() {
            group.programs.insert(input.program_id);
        }
        if !input.window_id.is_empty() {
            group.windows.insert(input.window_id);
        }
    }
    let truncated = grouped.len() > max_groups;
    let aggregates = grouped
        .into_values()
        .take(max_groups)
        .map(|group| FailureAggregate {
            category: group.category,
            method: group.method,
            error: group.error,
            count: group.count,
            signature_examples: group.signatures.into_iter().collect(),
            program_ids: group.programs.into_iter().collect(),
            window_ids: group.windows.into_iter().collect(),
        })
        .collect();
    (aggregates, total, truncated)
}

async fn build_manifest(
    config: &HistoryConfig,
    config_sha256: &str,
    checkpoint: &HistoryCheckpoint,
    checkpoint_path: &Path,
    tracker: &CorpusTracker,
    operation: &str,
    rebuild_derived_files: bool,
) -> anyhow::Result<CorpusManifest> {
    let root = PathBuf::from(&config.output_dir);
    let mut raw_rows = Vec::new();
    let mut min_slot = None;
    let mut max_slot = None;
    let mut observed_days = BTreeSet::new();
    for path in files_below(&root.join("raw"), "json")? {
        let value: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let slot = value
            .get("slot")
            .and_then(Value::as_str)
            .and_then(|slot| slot.parse::<u64>().ok())
            .unwrap_or_default();
        min_slot = Some(min_slot.map_or(slot, |current: u64| current.min(slot)));
        max_slot = Some(max_slot.map_or(slot, |current: u64| current.max(slot)));
        if let Some(day) = value
            .get("block_time")
            .and_then(Value::as_str)
            .and_then(|time| time.get(0..10))
        {
            observed_days.insert(day.to_string());
        }
        raw_rows.push(json!({
            "signature":value.get("signature").cloned().unwrap_or(Value::Null),
            "slot":slot.to_string(),
            "payload_sha256":value.get("payload_sha256").cloned().unwrap_or(Value::Null),
            "uri":path.strip_prefix(&root).unwrap_or(&path).to_string_lossy(),
        }));
    }
    raw_rows.sort_by_key(|row| {
        row.get("signature")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    let raw_index = jsonl_bytes(&raw_rows)?;
    let raw_index_path = root.join("raw-index.jsonl");
    if rebuild_derived_files {
        atomic_write(&raw_index_path, &raw_index).await?;
    }

    let mut events = Vec::new();
    for path in files_below(&root.join("canonical/by-event-id"), "json")? {
        events.push(serde_json::from_slice::<ChainEvent>(&std::fs::read(path)?)?);
    }
    events.sort_by(|a, b| {
        (a.slot.0, a.tx_index, &a.signature, a.event_index).cmp(&(
            b.slot.0,
            b.tx_index,
            &b.signature,
            b.event_index,
        ))
    });
    let canonical_rows = events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let canonical_jsonl = jsonl_bytes(&canonical_rows)?;
    let canonical_path = root.join("canonical/events.jsonl");
    if rebuild_derived_files {
        atomic_write(&canonical_path, &canonical_jsonl).await?;
    }

    let quarantine_count = events
        .iter()
        .filter(|event| matches!(event.payload, ChainEventPayload::DecodeQuarantined(_)))
        .count() as u64;
    let provider_inputs = checkpoint
        .streams
        .values()
        .flat_map(|stream| {
            stream
                .last_error
                .iter()
                .chain(
                    stream
                        .pending_transactions
                        .values()
                        .map(|pending| &pending.failure),
                )
                .map(|failure| AggregateInput {
                    category: "provider_or_fetch",
                    method: failure.method.clone(),
                    error: failure.error.clone(),
                    signature: failure.signature.clone(),
                    program_id: failure.program_id.clone(),
                    window_id: failure.window_id.clone(),
                    count: 1,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let (provider_failures, provider_failure_count, provider_truncated) = aggregate_failures(
        provider_inputs,
        config.max_failure_records.min(MAX_MANIFEST_FAILURE_GROUPS),
        config.failure_signature_examples,
    );
    let recovered_inputs = checkpoint
        .streams
        .values()
        .flat_map(|stream| {
            stream
                .recovered_transient_errors
                .values()
                .map(|recovered| AggregateInput {
                    category: "recovered_transient",
                    method: recovered.method.clone(),
                    error: recovered.error.clone(),
                    signature: None,
                    program_id: stream.program_id.clone(),
                    window_id: stream.window_id.clone(),
                    count: recovered.count,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let (recovered_transient_errors, recovered_transient_error_count, recovered_truncated) =
        aggregate_failures(
            recovered_inputs,
            config.max_failure_records.min(MAX_MANIFEST_FAILURE_GROUPS),
            config.failure_signature_examples,
        );

    let mut decode_inputs = Vec::new();
    let mut successfully_decoded_raw_count = 0u64;
    let mut schema_incompatible_raw_count = 0u64;
    let mut archived_not_decoded_count = 0u64;
    for signature in &tracker.raw_signatures {
        let marker_path = root.join("decode-state").join(format!("{signature}.json"));
        let Some(state) = read_decode_state(&marker_path) else {
            archived_not_decoded_count += 1;
            continue;
        };
        if state.decoder_version != DECODER_VERSION {
            archived_not_decoded_count += 1;
            continue;
        }
        match state.status {
            DecodeStatus::Success => successfully_decoded_raw_count += 1,
            DecodeStatus::SchemaIncompatible => schema_incompatible_raw_count += 1,
        }
        for issue in state.issues {
            decode_inputs.push(AggregateInput {
                category: "decode_schema_incompatibility",
                method: issue.method,
                error: issue.error,
                signature: Some(signature.clone()),
                program_id: issue.program_id,
                window_id: String::new(),
                count: 1,
            });
        }
    }
    let (decode_failures, decode_failure_count, decode_truncated) = aggregate_failures(
        decode_inputs,
        config.max_failure_records.min(MAX_MANIFEST_FAILURE_GROUPS),
        config.failure_signature_examples,
    );
    let failure_count = provider_failure_count + decode_failure_count;
    let failures_truncated = provider_truncated || decode_truncated || recovered_truncated;

    let mut completed_windows = Vec::new();
    let mut incomplete_windows = Vec::new();
    let mut configured_complete_days = 0u64;
    for window in &config.windows {
        configured_complete_days += (window.lte.date_naive() - window.gte.date_naive())
            .num_days()
            .unsigned_abs()
            + 1;
        if [PUMP_PROGRAM_ID, PUMPSWAP_PROGRAM_ID]
            .iter()
            .all(|program| {
                checkpoint
                    .streams
                    .get(&stream_key(program, &window.id))
                    .is_some_and(|stream| stream.complete)
            })
        {
            completed_windows.push(window.id.clone());
        } else {
            incomplete_windows.push(window.id.clone());
        }
    }
    let target_reached = tracker.launch_observations() >= config.target_launches;
    let raw_bound_reached = tracker.raw_signatures.len() >= config.max_raw_transactions;
    let request_bound_reached = checkpoint.total_requests >= config.max_requests;
    let bounded_complete = incomplete_windows.is_empty() && failure_count == 0 || target_reached;
    let raw_count = tracker.raw_signatures.len() as u64;
    let decode_success_fraction = if raw_count == 0 {
        0.0
    } else {
        successfully_decoded_raw_count as f64 / raw_count as f64
    };
    let decode_failure_fraction = if raw_count == 0 {
        1.0
    } else {
        (schema_incompatible_raw_count + archived_not_decoded_count) as f64 / raw_count as f64
    };
    let terminal_or_bounded = operation != "provider_backfill"
        || all_streams_complete(checkpoint)
        || target_reached
        || raw_bound_reached
        || request_bound_reached;
    let mut quality_gate_failures = Vec::new();
    if events.is_empty() {
        quality_gate_failures.push("canonical_event_count is zero".into());
    }
    if terminal_or_bounded {
        if decode_success_fraction < config.minimum_decode_success_fraction {
            quality_gate_failures.push(format!(
                "decode_success_fraction {:.6} is below minimum {:.6}",
                decode_success_fraction, config.minimum_decode_success_fraction
            ));
        }
        if decode_failure_fraction > config.maximum_decode_failure_fraction {
            quality_gate_failures.push(format!(
                "decode_failure_fraction {:.6} exceeds maximum {:.6}",
                decode_failure_fraction, config.maximum_decode_failure_fraction
            ));
        }
        if events.len() < config.minimum_canonical_events {
            quality_gate_failures.push(format!(
                "canonical_event_count {} is below minimum {}",
                events.len(),
                config.minimum_canonical_events
            ));
        }
        if tracker.launch_observations() < config.minimum_true_or_provisional_launches {
            quality_gate_failures.push(format!(
                "unique_launch_observations {} is below minimum {}",
                tracker.launch_observations(),
                config.minimum_true_or_provisional_launches
            ));
        }
    }
    quality_gate_failures.sort();
    quality_gate_failures.dedup();
    let failed_quality_gate = !quality_gate_failures.is_empty();
    let checkpoint_bytes = tokio::fs::read(checkpoint_path).await?;
    let mut events_by_protocol = tracker.events_by_protocol.clone();
    events_by_protocol.entry("pump_curve".into()).or_default();
    events_by_protocol.entry("pump_swap".into()).or_default();
    let manifest = CorpusManifest {
        version: MANIFEST_VERSION,
        generated_at: Utc::now(),
        operation: operation.into(),
        source: match checkpoint.effective_mode {
            HistoryMode::HeliusFull => "helius-getTransactionsForAddress",
            HistoryMode::StandardRpc => "solana-standard-json-rpc",
        }
        .into(),
        requested_mode: config.history_mode,
        effective_mode: checkpoint.effective_mode,
        fallback_reason: checkpoint.fallback_reason.clone(),
        programs: vec![PUMP_PROGRAM_ID.into(), PUMPSWAP_PROGRAM_ID.into()],
        raw_count,
        decoded_count: events.len() as u64 - quarantine_count,
        quarantine_count,
        canonical_event_count: events.len() as u64,
        true_launch_created_event_count: tracker.created_event_ids.len() as u64,
        true_launch_created_unique_mints: tracker.created_mints.len() as u64,
        provisional_first_trade_launch_count: tracker.provisional_first_trades() as u64,
        unique_launch_observations: tracker.launch_observations() as u64,
        unique_mints: tracker.unique_mints.len() as u64,
        events_by_protocol,
        min_slot: min_slot.map(|slot| slot.to_string()),
        max_slot: max_slot.map(|slot| slot.to_string()),
        decoder_version: DECODER_VERSION.into(),
        hashes: CorpusHashes {
            run_config_hash: config_sha256.into(),
            source_semantics_hash: checkpoint.source_semantics_hash.clone(),
            checkpoint_sha256: payload_sha256(&checkpoint_bytes),
            raw_index_sha256: payload_sha256(&raw_index),
            canonical_jsonl_sha256: payload_sha256(&canonical_jsonl),
        },
        credit_accounting: CreditAccounting {
            pricing_basis: "estimate: full results are currently documented at 10 credits per \
100 returned, rounded up with a 10-credit minimum; 100 credits is the conservative upper bound \
for a 1000-record call"
                .into(),
            successful_full_requests: checkpoint.helius_full_successful_requests,
            full_records_returned: checkpoint.helius_full_records_returned,
            metered_credits_estimate: checkpoint.helius_metered_credits_estimate,
            conservative_credits_per_request: FULL_CREDIT_UPPER_BOUND_PER_CALL,
            conservative_credits_upper_bound_used: checkpoint
                .helius_full_successful_requests
                .saturating_mul(FULL_CREDIT_UPPER_BOUND_PER_CALL),
            configured_maximum_credit_estimate: config.maximum_credit_estimate(),
        },
        calendar_coverage: CalendarCoverage {
            configured_window_count: config.windows.len(),
            configured_complete_days,
            earliest_gte: config.windows.first().expect("windows").gte,
            latest_lte: config.windows.last().expect("windows").lte,
            completed_windows,
            incomplete_windows,
            observed_calendar_days: observed_days.into_iter().collect(),
        },
        provider_failures,
        decode_failures,
        provider_failure_count,
        decode_failure_count,
        recovered_transient_errors,
        recovered_transient_error_count,
        decode_quality: DecodeQuality {
            archived_raw_count: raw_count,
            successfully_decoded_raw_count,
            schema_incompatible_raw_count,
            archived_not_decoded_count,
            decode_success_fraction,
            decode_failure_fraction,
            quality_gate_failures: quality_gate_failures.clone(),
        },
        failure_count,
        failures_truncated,
        completeness: Completeness {
            status: if failed_quality_gate {
                "failed_quality_gate".into()
            } else {
                checkpoint.global_stop_reason.clone().unwrap_or_else(|| {
                    if bounded_complete {
                        "configured_windows_complete".into()
                    } else {
                        "incomplete".into()
                    }
                })
            },
            bounded_scan_complete: bounded_complete,
            target_launches: config.target_launches,
            target_reached,
            raw_bound_reached,
            request_bound_reached,
            all_launches_complete: false,
            coverage_limitation: COVERAGE_LIMITATION.into(),
        },
        program_window_checkpoints: checkpoint.streams.clone(),
        canonical_events_uri: canonical_path.to_string_lossy().to_string(),
        raw_index_uri: raw_index_path.to_string_lossy().to_string(),
    };
    atomic_write(
        &root.join("corpus-manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )
    .await?;
    Ok(manifest)
}

fn files_below(root: &Path, extension: &str) -> anyhow::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn jsonl_bytes(rows: &[Value]) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut bytes, row)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use base64::Engine;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex as StdMutex,
    };

    #[derive(Default)]
    struct MockRpc {
        replies: StdMutex<BTreeMap<String, Vec<Value>>>,
        calls: StdMutex<Vec<Value>>,
        call_times: StdMutex<Vec<std::time::Instant>>,
        rate_limits_remaining: AtomicUsize,
        retry_after: StdMutex<Option<String>>,
    }

    fn request_key(body: &Value) -> String {
        let method = body.get("method").and_then(Value::as_str).unwrap_or("");
        let first = body
            .pointer("/params/0")
            .and_then(Value::as_str)
            .unwrap_or("");
        let cursor = match method {
            "getTransactionsForAddress" => body
                .pointer("/params/1/paginationToken")
                .and_then(Value::as_str)
                .unwrap_or(""),
            "getSignaturesForAddress" => body
                .pointer("/params/1/before")
                .and_then(Value::as_str)
                .unwrap_or(""),
            _ => "",
        };
        format!("{method}|{first}|{cursor}")
    }

    async fn mock_rpc(State(state): State<Arc<MockRpc>>, Json(body): Json<Value>) -> Response {
        state.calls.lock().unwrap().push(body.clone());
        state
            .call_times
            .lock()
            .unwrap()
            .push(std::time::Instant::now());
        if state
            .rate_limits_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":"rate limited"})),
            )
                .into_response();
            if let Some(value) = state.retry_after.lock().unwrap().as_deref() {
                response.headers_mut().insert(
                    reqwest::header::RETRY_AFTER,
                    value.parse().expect("valid Retry-After test value"),
                );
            }
            return response;
        }
        let key = request_key(&body);
        let mut replies = state.replies.lock().unwrap();
        let queue = replies.entry(key).or_insert_with(|| {
            vec![if body["method"] == "getTransactionsForAddress" {
                json!({"jsonrpc":"2.0","id":1,"result":{"data":[],"paginationToken":null}})
            } else {
                json!({"jsonrpc":"2.0","id":1,"result":[]})
            }]
        });
        let reply = if queue.len() > 1 {
            queue.remove(0)
        } else {
            queue.first().cloned().unwrap()
        };
        Json(reply).into_response()
    }

    async fn spawn_mock(state: Arc<MockRpc>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/", post(mock_rpc)).with_state(state);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), handle)
    }

    fn output(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pumphound-history-v2-{name}-{}",
            uuid::Uuid::now_v7()
        ))
    }

    fn config(output: &Path, mode: Option<HistoryMode>) -> HistoryConfig {
        HistoryConfig {
            version: 2,
            output_dir: output.to_string_lossy().to_string(),
            history_mode: mode,
            windows: vec![TimeWindow {
                id: "window-a".into(),
                gte: Utc.timestamp_opt(1_699_999_000, 0).unwrap(),
                lte: Utc.timestamp_opt(1_700_001_000, 0).unwrap(),
            }],
            window_strategy: WindowStrategy::RoundRobin,
            target_launches: 10,
            max_raw_transactions: 100,
            max_requests: 30,
            page_size: 2,
            before: None,
            until: Some("until-signature".into()),
            min_slot: None,
            max_slot: None,
            rate_limit_per_second: 10_000.0,
            retries: 0,
            backoff_ms: 1,
            concurrency: 2,
            request_timeout_secs: 5,
            max_supported_transaction_version: 0,
            commitment: "finalized".into(),
            max_failure_records: 100,
            allow_standard_fallback_on_denial: true,
            minimum_decode_success_fraction: 0.90,
            maximum_decode_failure_fraction: 0.10,
            minimum_canonical_events: 1,
            minimum_true_or_provisional_launches: 1,
            failure_signature_examples: 3,
        }
    }

    fn config_hash(config: &HistoryConfig) -> String {
        payload_sha256(&serde_json::to_vec(config).unwrap())
    }

    fn put_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn create_record(signature: &str, slot: u64) -> Value {
        let mut payload = Vec::new();
        put_string(&mut payload, "Historical");
        put_string(&mut payload, "HIST");
        put_string(&mut payload, "https://example.invalid/history.json");
        for _ in 0..4 {
            payload.extend_from_slice(&[0u8; 32]);
        }
        payload.extend_from_slice(&1_700_000_000i64.to_le_bytes());
        for value in [1u64, 2, 3, 4] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0u8; 32]);
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&[0u8; 32]);
        payload.extend_from_slice(&2u64.to_le_bytes());
        let mut event_data = vec![27, 114, 169, 77, 222, 235, 99, 118];
        event_data.extend(payload);
        let encoded = base64::engine::general_purpose::STANDARD.encode(event_data);
        json!({
            "slot":slot,
            "transactionIndex":7,
            "blockTime":1_700_000_000,
            "transaction":{
                "signatures":[signature],
                "message":{
                    "header":{"numRequiredSignatures":1,"numReadonlySignedAccounts":0,"numReadonlyUnsignedAccounts":1},
                    "accountKeys":["11111111111111111111111111111111",PUMP_PROGRAM_ID],
                    "recentBlockhash":"11111111111111111111111111111111",
                    "instructions":[{"programIdIndex":1,"accounts":[0],"data":""}]
                }
            },
            "meta":{
                "err":null,
                "fee":5000,
                "preBalances":[0,0],
                "postBalances":[0,0],
                "preTokenBalances":[],
                "postTokenBalances":[],
                "innerInstructions":[],
                "logMessages":[
                    format!("Program {PUMP_PROGRAM_ID} invoke [1]"),
                    format!("Program data: {encoded}"),
                    format!("Program {PUMP_PROGRAM_ID} success")
                ]
            },
            "version":0
        })
    }

    fn signature(signature: &str, slot: u64) -> Value {
        json!({"signature":signature,"slot":slot,"blockTime":1_700_000_000,"err":null})
    }

    fn result(data: Vec<Value>, token: Option<&str>) -> Value {
        json!({"jsonrpc":"2.0","id":1,"result":{"data":data,"paginationToken":token}})
    }

    fn add_reply(state: &Arc<MockRpc>, key: String, reply: Value) {
        state.replies.lock().unwrap().insert(key, vec![reply]);
    }

    #[tokio::test]
    async fn helius_full_uses_exact_filters_token_and_raw_conversion() {
        let output = output("full");
        let config = config(&output, None);
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
            result(vec![create_record("sig-a", 10)], Some("10:7")),
        );
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|10:7"),
            result(vec![], None),
        );
        let (url, server) = spawn_mock(state.clone()).await;
        let manifest = run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert_eq!(manifest.effective_mode, HistoryMode::HeliusFull);
        assert_eq!(manifest.raw_count, 1);
        assert_eq!(manifest.true_launch_created_event_count, 1);
        assert_eq!(manifest.unique_launch_observations, 1);
        assert_eq!(manifest.events_by_protocol["pump_curve"], 1);
        assert_eq!(manifest.events_by_protocol["pump_swap"], 0);
        assert!(ensure_quality(&manifest).is_ok());
        assert_eq!(
            manifest
                .credit_accounting
                .conservative_credits_upper_bound_used,
            manifest.credit_accounting.successful_full_requests * 100
        );
        let calls = state.calls.lock().unwrap();
        let first = calls
            .iter()
            .find(|call| call["method"] == "getTransactionsForAddress")
            .unwrap();
        assert_eq!(first["params"][1]["transactionDetails"], "full");
        assert_eq!(first["params"][1]["sortOrder"], "asc");
        assert_eq!(first["params"][1]["encoding"], "json");
        assert_eq!(first["params"][1]["commitment"], "finalized");
        assert_eq!(first["params"][1]["maxSupportedTransactionVersion"], 0);
        assert_eq!(first["params"][1]["filters"]["status"], "succeeded");
        assert_eq!(
            first["params"][1]["filters"]["blockTime"]["gte"],
            config.windows[0].gte.timestamp()
        );
        assert!(calls
            .iter()
            .any(|call| { call.pointer("/params/1/paginationToken") == Some(&json!("10:7")) }));
        let event: ChainEvent = serde_json::from_str(
            std::fs::read_to_string(output.join("canonical/events.jsonl"))
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event.tx_index, Some(7));
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn helius_full_resume_uses_saved_pagination_token() {
        let output = output("resume");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
            result(vec![create_record("resume-a", 10)], Some("next-token")),
        );
        state.replies.lock().unwrap().insert(
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|next-token"),
            vec![
                json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"temporary"}}),
                result(vec![], None),
            ],
        );
        let (url, server) = spawn_mock(state.clone()).await;
        let hash = config_hash(&config);
        let first = run_with_rpc(
            config.clone(),
            hash.clone(),
            url.clone(),
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert!(first.failure_count > 0);
        let second = run_with_rpc(config, hash, url, Some("secret".into()))
            .await
            .unwrap();
        assert!(
            second
                .program_window_checkpoints
                .get(&stream_key(PUMP_PROGRAM_ID, "window-a"))
                .unwrap()
                .complete
        );
        let pump_calls = state
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| {
                call["method"] == "getTransactionsForAddress"
                    && call["params"][0] == PUMP_PROGRAM_ID
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            pump_calls
                .iter()
                .filter(|call| call.pointer("/params/1/paginationToken").is_none())
                .count(),
            1
        );
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn launch_target_stops_inside_full_page() {
        let output = output("target");
        let mut config = config(&output, Some(HistoryMode::HeliusFull));
        config.target_launches = 1;
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
            result(
                vec![create_record("target-a", 10), create_record("target-b", 11)],
                Some("unused"),
            ),
        );
        let (url, server) = spawn_mock(state).await;
        let manifest = run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert_eq!(manifest.unique_launch_observations, 1);
        assert_eq!(manifest.raw_count, 1);
        assert!(manifest.completeness.target_reached);
        assert_eq!(manifest.completeness.status, "target_launches_reached");
        assert_eq!(
            manifest.calendar_coverage.incomplete_windows,
            vec!["window-a"]
        );
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn plan_denial_falls_back_only_when_allowed() {
        async fn one(allow: bool) -> (CorpusManifest, Vec<Value>, PathBuf) {
            let output = output(if allow { "fallback" } else { "fail-closed" });
            let mut config = config(&output, None);
            config.allow_standard_fallback_on_denial = allow;
            let state = Arc::new(MockRpc::default());
            add_reply(
                &state,
                format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
                json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not available on your plan"}}),
            );
            let (url, server) = spawn_mock(state.clone()).await;
            let manifest = run_with_rpc(
                config.clone(),
                config_hash(&config),
                url,
                Some("secret".into()),
            )
            .await
            .unwrap();
            server.abort();
            let calls = state.calls.lock().unwrap().clone();
            (manifest, calls, output)
        }
        let (fallback, calls, fallback_output) = one(true).await;
        assert_eq!(fallback.effective_mode, HistoryMode::StandardRpc);
        assert!(fallback.fallback_reason.is_some());
        assert!(calls
            .iter()
            .any(|call| call["method"] == "getSignaturesForAddress"));
        let (closed, calls, closed_output) = one(false).await;
        assert_eq!(closed.effective_mode, HistoryMode::HeliusFull);
        assert!(closed.failure_count > 0);
        assert!(!calls
            .iter()
            .any(|call| call["method"] == "getSignaturesForAddress"));
        let _ = std::fs::remove_dir_all(fallback_output);
        let _ = std::fs::remove_dir_all(closed_output);
    }

    #[tokio::test]
    async fn explicit_standard_rpc_preserves_pagination_retry_and_resume_contract() {
        let output = output("standard");
        let config = config(&output, Some(HistoryMode::StandardRpc));
        let state = Arc::new(MockRpc::default());
        state.rate_limits_remaining.store(1, Ordering::SeqCst);
        add_reply(
            &state,
            format!("getSignaturesForAddress|{PUMP_PROGRAM_ID}|"),
            json!({"jsonrpc":"2.0","id":1,"result":[signature("std-a",10),signature("std-b",11)]}),
        );
        add_reply(
            &state,
            format!("getSignaturesForAddress|{PUMP_PROGRAM_ID}|std-b"),
            json!({"jsonrpc":"2.0","id":1,"result":[]}),
        );
        for signature in ["std-a", "std-b"] {
            add_reply(
                &state,
                format!("getTransaction|{signature}|"),
                json!({"jsonrpc":"2.0","id":1,"result":create_record(signature,10)}),
            );
        }
        let (url, server) = spawn_mock(state.clone()).await;
        let manifest = run_with_rpc(config.clone(), config_hash(&config), url, None)
            .await
            .unwrap();
        assert_eq!(manifest.effective_mode, HistoryMode::StandardRpc);
        assert_eq!(manifest.raw_count, 2);
        let calls = state.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|call| { call.pointer("/params/1/before") == Some(&json!("std-b")) }));
        assert!(calls
            .iter()
            .filter(|call| call["method"] == "getSignaturesForAddress")
            .all(|call| call.pointer("/params/1/until") == Some(&json!("until-signature"))));
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn standard_rpc_retries_pending_transaction_after_resume() {
        let output = output("standard-resume");
        let mut config = config(&output, Some(HistoryMode::StandardRpc));
        config.page_size = 10;
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getSignaturesForAddress|{PUMP_PROGRAM_ID}|"),
            json!({"jsonrpc":"2.0","id":1,"result":[signature("pending-standard",10)]}),
        );
        state.replies.lock().unwrap().insert(
            "getTransaction|pending-standard|".into(),
            vec![
                json!({"jsonrpc":"2.0","id":1,"result":null}),
                json!({"jsonrpc":"2.0","id":1,"result":create_record("pending-standard",10)}),
            ],
        );
        let (url, server) = spawn_mock(state).await;
        let hash = config_hash(&config);
        let first = run_with_rpc(config.clone(), hash.clone(), url.clone(), None)
            .await
            .unwrap();
        assert_eq!(first.raw_count, 0);
        assert_eq!(first.failure_count, 1);
        let second = run_with_rpc(config, hash, url, None).await.unwrap();
        assert_eq!(second.raw_count, 1);
        assert_eq!(second.failure_count, 0);
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn canonical_hashes_are_deterministic_in_full_mode() {
        async fn one(output: &Path) -> CorpusManifest {
            let config = config(output, Some(HistoryMode::HeliusFull));
            let state = Arc::new(MockRpc::default());
            add_reply(
                &state,
                format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
                result(vec![create_record("deterministic", 10)], None),
            );
            let (url, server) = spawn_mock(state).await;
            let manifest = run_with_rpc(
                config.clone(),
                config_hash(&config),
                url,
                Some("secret".into()),
            )
            .await
            .unwrap();
            server.abort();
            manifest
        }
        let first_output = output("hash-a");
        let second_output = output("hash-b");
        let first = one(&first_output).await;
        let second = one(&second_output).await;
        assert_eq!(
            first.hashes.canonical_jsonl_sha256,
            second.hashes.canonical_jsonl_sha256
        );
        assert_eq!(
            first.hashes.raw_index_sha256,
            second.hashes.raw_index_sha256
        );
        let _ = std::fs::remove_dir_all(first_output);
        let _ = std::fs::remove_dir_all(second_output);
    }

    #[tokio::test]
    async fn zero_decode_manifest_fails_quality_and_exit_guard() {
        let output = output("zero-quality");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let state = Arc::new(MockRpc::default());
        let (url, server) = spawn_mock(state).await;
        let manifest = run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert_eq!(manifest.canonical_event_count, 0);
        assert_eq!(manifest.completeness.status, "failed_quality_gate");
        assert!(ensure_quality(&manifest).is_err());
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn incompatible_archived_schema_stops_before_provider_and_is_decode_failure() {
        let output = output("decode-preflight");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let incompatible_transaction = json!({"unsupported_historical_schema":true});
        let raw_hash = payload_sha256(&serde_json::to_vec(&incompatible_transaction).unwrap());
        let raw_path = output.join("raw/date=2023-11-14/incompatible.json");
        std::fs::create_dir_all(raw_path.parent().unwrap()).unwrap();
        std::fs::write(
            &raw_path,
            serde_json::to_vec_pretty(&json!({
                "signature":"incompatible",
                "slot":"10",
                "block_time":"2023-11-14T22:13:20Z",
                "payload_sha256":raw_hash,
                "transaction":incompatible_transaction
            }))
            .unwrap(),
        )
        .unwrap();
        let state = Arc::new(MockRpc::default());
        let (url, server) = spawn_mock(state.clone()).await;
        let manifest = run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert!(state.calls.lock().unwrap().is_empty());
        assert_eq!(manifest.provider_failure_count, 0);
        assert_eq!(manifest.decode_failure_count, 1);
        assert_eq!(
            manifest.decode_failures[0].category,
            "decode_schema_incompatibility"
        );
        assert_eq!(manifest.completeness.status, "failed_quality_gate");
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[test]
    fn repeated_errors_are_aggregated_with_bounded_examples() {
        let inputs = (0..20)
            .map(|index| AggregateInput {
                category: "decode_schema_incompatibility",
                method: "decode:pump".into(),
                error: "unsupported historical schema".into(),
                signature: Some(format!("sig-{index:02}")),
                program_id: PUMP_PROGRAM_ID.into(),
                window_id: "window-a".into(),
                count: 1,
            })
            .collect();
        let (aggregates, total, truncated) = aggregate_failures(inputs, 10, 3);
        assert_eq!(total, 20);
        assert!(!truncated);
        assert_eq!(aggregates.len(), 1);
        assert_eq!(aggregates[0].count, 20);
        assert_eq!(aggregates[0].signature_examples.len(), 3);
    }

    #[tokio::test]
    async fn global_limiter_is_shared_across_two_windows() {
        let output = output("global-limiter");
        let mut config = config(&output, Some(HistoryMode::HeliusFull));
        config.windows.push(TimeWindow {
            id: "window-b".into(),
            gte: Utc.timestamp_opt(1_700_002_000, 0).unwrap(),
            lte: Utc.timestamp_opt(1_700_003_000, 0).unwrap(),
        });
        config.rate_limit_per_second = 20.0;
        let state = Arc::new(MockRpc::default());
        let (url, server) = spawn_mock(state.clone()).await;
        run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();

        let times = state.call_times.lock().unwrap();
        assert_eq!(times.len(), 4);
        assert!(times
            .windows(2)
            .all(|pair| pair[1].duration_since(pair[0]) >= Duration::from_millis(40)));
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn retry_after_is_honored_and_recorded() {
        let output = output("retry-after");
        let mut config = config(&output, Some(HistoryMode::HeliusFull));
        config.retries = 1;
        config.rate_limit_per_second = 10_000.0;
        let state = Arc::new(MockRpc::default());
        state.rate_limits_remaining.store(1, Ordering::SeqCst);
        *state.retry_after.lock().unwrap() = Some("1".into());
        let (url, server) = spawn_mock(state).await;
        let rpc = RpcClient::new(url, Some("secret".into()), &config).unwrap();
        let started = std::time::Instant::now();
        let result = rpc.call("testMethod", json!([])).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(900));
        assert_eq!(result.transient_attempts.len(), 1);
        assert!(result.transient_attempts[0].retry_after_used);

        let future = (Utc::now() + chrono::Duration::seconds(2))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        assert!(parse_retry_after(&future).is_some());
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn recovered_429_is_not_an_unresolved_provider_failure() {
        let output = output("recovered-429");
        let mut config = config(&output, Some(HistoryMode::HeliusFull));
        config.max_requests = 10;
        let state = Arc::new(MockRpc::default());
        state.rate_limits_remaining.store(1, Ordering::SeqCst);
        let (url, server) = spawn_mock(state).await;
        let manifest = run_with_rpc(
            config.clone(),
            config_hash(&config),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap();
        assert_eq!(manifest.provider_failure_count, 0);
        assert_eq!(manifest.recovered_transient_error_count, 1);
        assert!(manifest
            .program_window_checkpoints
            .values()
            .all(|stream| stream.last_error.is_none()));
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn operational_config_changes_resume_but_source_changes_do_not() {
        let output = output("resume-hashes");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let state = Arc::new(MockRpc::default());
        let (url, server) = spawn_mock(state).await;
        run_with_rpc(
            config.clone(),
            config_hash(&config),
            url.clone(),
            Some("secret".into()),
        )
        .await
        .unwrap();

        let mut operational = config.clone();
        operational.rate_limit_per_second = 1.0;
        operational.concurrency = 1;
        operational.retries = 20;
        operational.backoff_ms = 3_000;
        operational.request_timeout_secs = 60;
        operational.minimum_decode_success_fraction = 0.5;
        operational.maximum_decode_failure_fraction = 0.5;
        run_with_rpc(
            operational.clone(),
            config_hash(&operational),
            url.clone(),
            Some("secret".into()),
        )
        .await
        .unwrap();

        let mut changed_source = operational;
        changed_source.windows[0].lte += chrono::Duration::seconds(1);
        let error = run_with_rpc(
            changed_source.clone(),
            config_hash(&changed_source),
            url,
            Some("secret".into()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("source semantics changed"));
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn redecode_rebuilds_derived_files_without_http_or_raw_writes() {
        let output = output("offline-redecode");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
            result(vec![create_record("offline", 10)], None),
        );
        let (url, server) = spawn_mock(state.clone()).await;
        let hash = config_hash(&config);
        run_with_rpc(config.clone(), hash.clone(), url, Some("secret".into()))
            .await
            .unwrap();
        let calls_before = state.calls.lock().unwrap().len();
        let raw_path = files_below(&output.join("raw"), "json").unwrap().remove(0);
        let raw_before = std::fs::read(&raw_path).unwrap();
        let checkpoint_path = output.join("checkpoint.json");
        let mut checkpoint: Value =
            serde_json::from_slice(&std::fs::read(&checkpoint_path).unwrap()).unwrap();
        let stream = stream_key(PUMP_PROGRAM_ID, "window-a");
        checkpoint["streams"][&stream]["pagination_token"] = json!("saved-provider-token");
        std::fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&checkpoint).unwrap(),
        )
        .unwrap();

        let manifest = redecode(config, hash).await.unwrap();
        assert_eq!(manifest.operation, "redecode");
        assert_eq!(manifest.canonical_event_count, 1);
        assert!(ensure_quality(&manifest).is_ok());
        assert_eq!(state.calls.lock().unwrap().len(), calls_before);
        assert_eq!(std::fs::read(raw_path).unwrap(), raw_before);
        let checkpoint: Value =
            serde_json::from_slice(&std::fs::read(checkpoint_path).unwrap()).unwrap();
        assert_eq!(
            checkpoint["streams"][stream]["pagination_token"],
            "saved-provider-token"
        );
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn stale_decoder_marker_reprocesses_raw_without_provider_call() {
        let output = output("decoder-invalidation");
        let config = config(&output, Some(HistoryMode::HeliusFull));
        let state = Arc::new(MockRpc::default());
        add_reply(
            &state,
            format!("getTransactionsForAddress|{PUMP_PROGRAM_ID}|"),
            result(vec![create_record("stale-decoder", 10)], None),
        );
        let (url, server) = spawn_mock(state.clone()).await;
        let hash = config_hash(&config);
        run_with_rpc(
            config.clone(),
            hash.clone(),
            url.clone(),
            Some("secret".into()),
        )
        .await
        .unwrap();
        let calls_before = state.calls.lock().unwrap().len();
        let marker_path = output.join("decode-state/stale-decoder.json");
        let mut marker: Value =
            serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
        marker["decoder_version"] = json!("old-decoder");
        std::fs::write(&marker_path, serde_json::to_vec_pretty(&marker).unwrap()).unwrap();
        for event in files_below(&output.join("canonical/by-event-id"), "json").unwrap() {
            std::fs::remove_file(event).unwrap();
        }

        let manifest = run_with_rpc(config, hash, url, Some("secret".into()))
            .await
            .unwrap();
        let refreshed: DecodeState =
            serde_json::from_slice(&std::fs::read(marker_path).unwrap()).unwrap();
        assert_eq!(refreshed.decoder_version, DECODER_VERSION);
        assert_eq!(manifest.canonical_event_count, 1);
        assert_eq!(state.calls.lock().unwrap().len(), calls_before);
        server.abort();
        let _ = std::fs::remove_dir_all(output);
    }

    #[tokio::test]
    async fn large_config_migrates_the_paid_checkpoint_hash() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/history-large-25k.json");
        let (config, hash) = HistoryConfig::load(&path).unwrap();
        assert_eq!(config.rate_limit_per_second, 1.0);
        assert_eq!(config.concurrency, 1);
        assert_eq!(config.retries, 20);
        assert_eq!(config.backoff_ms, 3_000);

        let checkpoint_path = output("paid-hash-migration").join("checkpoint.json");
        let mut checkpoint = HistoryCheckpoint {
            version: CHECKPOINT_VERSION,
            run_config_hash: PAID_CORPUS_PRE_SEMANTICS_HASH.into(),
            source_semantics_hash: String::new(),
            decoder_version: DECODER_VERSION.into(),
            requested_mode: config.history_mode,
            effective_mode: HistoryMode::HeliusFull,
            fallback_reason: None,
            total_requests: 22,
            helius_full_successful_requests: 20,
            helius_full_records_returned: 8_603,
            helius_metered_credits_estimate: 970,
            global_stop_reason: Some("rpc_error".into()),
            streams: BTreeMap::new(),
            decode_failures: BTreeMap::new(),
        };
        ensure_streams(&mut checkpoint, &config);
        save_checkpoint(&checkpoint_path, &checkpoint)
            .await
            .unwrap();
        let migrated = load_checkpoint(&checkpoint_path, &hash, HistoryMode::HeliusFull, &config)
            .await
            .unwrap();
        assert_eq!(migrated.run_config_hash, hash);
        assert_eq!(
            migrated.source_semantics_hash,
            source_semantics_hash(&config).unwrap()
        );
        let _ = std::fs::remove_dir_all(checkpoint_path.parent().unwrap());
    }

    #[test]
    fn rejects_malformed_full_response_shape() {
        assert!(parse_full_page(json!({"data":[]}), 100).is_err());
        assert!(parse_full_page(json!({"data":"wrong","paginationToken":null}), 100).is_err());
    }
}
