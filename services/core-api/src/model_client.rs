use pumphound_domain::FeatureVector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const REQUIRED_TARGETS: [&str; 4] = [
    "graduated_by_6h_v1",
    "up_50_before_down_20_15m_v1",
    "severe_collapse_1h_v1",
    "exit_hazard_60s_v1",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetChampion {
    pub model_version: String,
    pub feature_schema_version: String,
    pub manifest: Value,
}

pub type ChampionSet = BTreeMap<String, TargetChampion>;

pub fn missing_required_targets(champions: &ChampionSet) -> Vec<String> {
    REQUIRED_TARGETS
        .iter()
        .filter(|target| !champions.contains_key(**target))
        .map(|target| (*target).to_string())
        .collect()
}

pub fn target_version_map(champions: &ChampionSet) -> BTreeMap<String, String> {
    champions
        .iter()
        .map(|(target, champion)| (target.clone(), champion.model_version.clone()))
        .collect()
}

pub fn champions_compatible(champions: &ChampionSet, feature_schema: &str) -> Result<(), String> {
    let missing = missing_required_targets(champions);
    if !missing.is_empty() {
        return Err(format!("missing_champions:{}", missing.join(",")));
    }
    for target in REQUIRED_TARGETS {
        let champion = &champions[target];
        if champion.feature_schema_version != feature_schema {
            return Err(format!("champion_schema_mismatch:{target}"));
        }
        let manifest_schema = champion
            .manifest
            .get("feature_schema_version")
            .and_then(Value::as_str);
        if manifest_schema != Some(feature_schema) {
            return Err(format!("champion_manifest_schema_mismatch:{target}"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ModelClient {
    http: reqwest::Client,
    predict_url: String,
}

#[derive(Clone, Debug, Serialize)]
struct PredictRequest<'a> {
    feature_vector: &'a FeatureVector,
    timeout_s: f64,
    authority: &'static str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AggregatePrediction {
    pub status: String,
    #[serde(default)]
    pub authority: Option<String>,
    pub prediction_id: String,
    pub feature_snapshot_id: String,
    pub launch_id: String,
    pub observation_window_seconds: u32,
    #[serde(default)]
    pub probabilities: BTreeMap<String, Option<f64>>,
    #[serde(default)]
    pub target_model_versions: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub familiarity: BTreeMap<String, Option<f64>>,
    #[serde(default)]
    pub withheld_reasons: BTreeMap<String, Value>,
    #[serde(default)]
    pub explanations: BTreeMap<String, Value>,
}

impl AggregatePrediction {
    pub fn probability(&self, target: &str) -> Option<f64> {
        self.probabilities.get(target).copied().flatten()
    }

    /// Validate one consumer target independently of unrelated model targets.
    pub fn validate_target(
        &self,
        target: &str,
        vector: &FeatureVector,
        champions: &ChampionSet,
    ) -> Result<(), String> {
        let champion = champions
            .get(target)
            .ok_or_else(|| format!("missing_champion:{target}"))?;
        if self.authority.as_deref() != Some("CONTROL") {
            return Err("prediction_control_authority_unverified".into());
        }
        if self.feature_snapshot_id != vector.feature_snapshot_id
            || self.launch_id != vector.launch_id
            || self.observation_window_seconds != vector.observation_window_seconds
        {
            return Err("prediction_input_identity_mismatch".into());
        }
        if champion.feature_schema_version != vector.schema_version
            || champion
                .manifest
                .get("feature_schema_version")
                .and_then(Value::as_str)
                != Some(vector.schema_version.as_str())
        {
            return Err(format!("champion_schema_mismatch:{target}"));
        }
        if self
            .target_model_versions
            .get(target)
            .and_then(|v| v.as_deref())
            != Some(champion.model_version.as_str())
        {
            return Err(format!("target_version_mismatch:{target}"));
        }
        if let Some(reason) = self.withheld_reasons.get(target).filter(|r| !r.is_null()) {
            return Err(format!("prediction_withheld:{target}:{reason}"));
        }
        let value = self
            .probability(target)
            .ok_or_else(|| format!("probability_missing:{target}"))?;
        if !value.is_finite() || (target != "expected_mfe_5m_v1" && !(0.0..=1.0).contains(&value)) {
            return Err(format!("probability_invalid:{target}"));
        }
        let familiarity = self.familiarity.get(target).copied().flatten();
        if !familiarity.is_some_and(|f| f.is_finite() && (0.0..=1.0).contains(&f)) {
            return Err(format!("familiarity_missing:{target}"));
        }
        Ok(())
    }

    pub fn minimum_familiarity(&self) -> Option<f64> {
        let scores: Vec<f64> = self.familiarity.values().copied().flatten().collect();
        if scores.is_empty() {
            None
        } else {
            scores.into_iter().reduce(f64::min)
        }
    }

    pub fn validate_for(
        &self,
        vector: &FeatureVector,
        champions: &ChampionSet,
        minimum_familiarity: f64,
    ) -> Result<(), String> {
        champions_compatible(champions, &vector.schema_version)?;
        if self.feature_snapshot_id != vector.feature_snapshot_id {
            return Err("prediction_snapshot_mismatch".into());
        }
        if self.launch_id != vector.launch_id {
            return Err("prediction_launch_mismatch".into());
        }
        if self.observation_window_seconds != vector.observation_window_seconds {
            return Err("prediction_window_mismatch".into());
        }
        if !self.withheld_reasons.is_empty() {
            let targets = self
                .withheld_reasons
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            return Err(format!("withheld_targets:{targets}"));
        }
        if self.status != "OK" {
            return Err(format!("model_bundle_status:{}", self.status));
        }
        if !has_exact_targets(&self.probabilities) {
            return Err("probability_target_set_mismatch".into());
        }
        if !has_exact_targets(&self.target_model_versions) {
            return Err("target_version_set_mismatch".into());
        }
        if !has_exact_targets(&self.familiarity) {
            return Err("familiarity_target_set_mismatch".into());
        }
        let expected_versions = target_version_map(champions);
        for target in REQUIRED_TARGETS {
            let probability = self
                .probability(target)
                .ok_or_else(|| format!("probability_missing:{target}"))?;
            if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
                return Err(format!("probability_invalid:{target}"));
            }
            let returned_version = self
                .target_model_versions
                .get(target)
                .and_then(|value| value.as_deref())
                .ok_or_else(|| format!("target_version_missing:{target}"))?;
            if expected_versions.get(target).map(String::as_str) != Some(returned_version) {
                return Err(format!("target_version_mismatch:{target}"));
            }
            let familiarity = self
                .familiarity
                .get(target)
                .copied()
                .flatten()
                .ok_or_else(|| format!("familiarity_withheld:{target}"))?;
            if !familiarity.is_finite() || familiarity <= minimum_familiarity {
                return Err(format!("familiarity_below_policy:{target}"));
            }
        }
        Ok(())
    }
}

fn has_exact_targets<T>(values: &BTreeMap<String, T>) -> bool {
    values.len() == REQUIRED_TARGETS.len()
        && REQUIRED_TARGETS
            .iter()
            .all(|target| values.contains_key(*target))
}

impl ModelClient {
    pub fn from_env() -> Self {
        let configured_predict_url = std::env::var("PUMPHOUND_ML_PREDICT_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8091/predict".into());
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            predict_url: normalize_predict_url(&configured_predict_url),
        }
    }

    pub async fn predict(&self, vector: &FeatureVector) -> Result<AggregatePrediction, String> {
        self.predict_with_authority(vector, "control").await
    }

    pub async fn predict_shadow(
        &self,
        vector: &FeatureVector,
    ) -> Result<AggregatePrediction, String> {
        self.predict_with_authority(vector, "shadow").await
    }

    async fn predict_with_authority(
        &self,
        vector: &FeatureVector,
        authority: &'static str,
    ) -> Result<AggregatePrediction, String> {
        let response = self
            .http
            .post(&self.predict_url)
            .json(&PredictRequest {
                feature_vector: vector,
                timeout_s: 2.0,
                authority,
            })
            .send()
            .await
            .map_err(|e| format!("model_unavailable:{e}"))?;
        if !response.status().is_success() {
            return Err(format!("model_http_status:{}", response.status()));
        }
        response
            .json::<AggregatePrediction>()
            .await
            .map_err(|e| format!("invalid_model_response:{e}"))
    }
}

fn normalize_predict_url(configured: &str) -> String {
    let configured = configured.trim_end_matches('/');
    configured
        .strip_suffix("/shadow")
        .unwrap_or(configured)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumphound_domain::{
        Commitment, Coverage, DataQuality, ObservationPoint, OrderingQuality, QualityGrade, Slot,
    };
    use serde_json::json;

    fn vector() -> FeatureVector {
        FeatureVector {
            feature_snapshot_id: "snapshot".into(),
            launch_id: "launch".into(),
            observation_window_seconds: 60,
            as_of: ObservationPoint {
                slot: Slot(1),
                tx_index: None,
                event_index: None,
                event_time: Some(chrono::Utc::now()),
                observed_at: chrono::Utc::now(),
                knowledge_cutoff: chrono::Utc::now(),
                commitment: Commitment::Confirmed,
            },
            schema_version: "features-v1".into(),
            values: Default::default(),
            dependency_ids: vec![],
            quality: DataQuality {
                grade: QualityGrade::A,
                score: 1.0,
                chain_lag_ms: Some(0),
                market_lag_ms: Some(0),
                ordering: OrderingQuality::Exact,
                coverage: Coverage::CompleteForScope,
                missing_fields: vec![],
                stale_components: vec![],
                reasons: vec![],
            },
        }
    }

    fn champions() -> ChampionSet {
        REQUIRED_TARGETS
            .iter()
            .map(|target| {
                (
                    (*target).into(),
                    TargetChampion {
                        model_version: format!("{target}-champion"),
                        feature_schema_version: "features-v1".into(),
                        manifest: json!({"feature_schema_version": "features-v1"}),
                    },
                )
            })
            .collect()
    }

    fn bundle() -> AggregatePrediction {
        serde_json::from_value(json!({
            "status": "OK",
            "authority": "CONTROL",
            "prediction_id": "aggregate-prediction",
            "feature_snapshot_id": "snapshot",
            "launch_id": "launch",
            "observation_window_seconds": 60,
            "probabilities": {
                "graduated_by_6h_v1": 0.8,
                "up_50_before_down_20_15m_v1": 0.7,
                "severe_collapse_1h_v1": 0.1,
                "exit_hazard_60s_v1": 0.2
            },
            "target_model_versions": {
                "graduated_by_6h_v1": "graduated_by_6h_v1-champion",
                "up_50_before_down_20_15m_v1": "up_50_before_down_20_15m_v1-champion",
                "severe_collapse_1h_v1": "severe_collapse_1h_v1-champion",
                "exit_hazard_60s_v1": "exit_hazard_60s_v1-champion"
            },
            "familiarity": {
                "graduated_by_6h_v1": 0.9,
                "up_50_before_down_20_15m_v1": 0.9,
                "severe_collapse_1h_v1": 0.9,
                "exit_hazard_60s_v1": 0.9
            },
            "withheld_reasons": {},
            "explanations": {}
        }))
        .unwrap()
    }

    #[test]
    fn missing_one_champion_is_exact() {
        let mut set = champions();
        set.remove("exit_hazard_60s_v1");
        assert_eq!(missing_required_targets(&set), vec!["exit_hazard_60s_v1"]);
    }

    #[test]
    fn mismatched_target_version_is_rejected() {
        let mut prediction = bundle();
        prediction
            .target_model_versions
            .insert("severe_collapse_1h_v1".into(), Some("wrong-version".into()));
        assert_eq!(
            prediction.validate_for(&vector(), &champions(), 0.5),
            Err("target_version_mismatch:severe_collapse_1h_v1".into())
        );
    }

    #[test]
    fn withheld_one_target_is_rejected() {
        let mut prediction = bundle();
        prediction.status = "MODEL WITHHELD".into();
        prediction
            .withheld_reasons
            .insert("exit_hazard_60s_v1".into(), json!("unfamiliar"));
        assert_eq!(
            prediction.validate_for(&vector(), &champions(), 0.5),
            Err("withheld_targets:exit_hazard_60s_v1".into())
        );
    }

    #[test]
    fn all_target_bundle_is_accepted() {
        assert!(bundle().validate_for(&vector(), &champions(), 0.5).is_ok());
    }
    #[test]
    fn short_target_validation_does_not_require_long_horizon_models() {
        let mut p = bundle();
        let mut set = champions();
        let target = "graduated_by_6h_v1";
        set.retain(|key, _| key == target);
        p.withheld_reasons
            .insert("exit_hazard_60s_v1".into(), json!("missing_champion"));
        p.status = "MODEL WITHHELD".into();
        assert!(p.validate_target(target, &vector(), &set).is_ok());
    }

    #[test]
    fn target_validation_rejects_shadow_or_wrong_version() {
        let mut p = bundle();
        p.target_model_versions
            .insert("graduated_by_6h_v1".into(), Some("shadow".into()));
        assert_eq!(
            p.validate_target("graduated_by_6h_v1", &vector(), &champions()),
            Err("target_version_mismatch:graduated_by_6h_v1".into())
        );
    }

    #[test]
    fn legacy_shadow_endpoint_is_normalized_for_authority_routing() {
        assert_eq!(
            normalize_predict_url("http://127.0.0.1:8091/predict/shadow"),
            "http://127.0.0.1:8091/predict"
        );
        assert_eq!(
            normalize_predict_url("http://127.0.0.1:8091/predict/"),
            "http://127.0.0.1:8091/predict"
        );
    }
}
