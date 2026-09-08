use pumphound_domain::{now_utc, DataQuality, FeatureVector, ObservationPoint};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub const OBSERVATION_WINDOWS: [u32; 6] = [5, 15, 30, 60, 120, 300];

pub const MARKET_FEATURE_NAMES: &[&str] = &[
    "age_seconds",
    "curve_progress",
    "round_trip_cost_fraction",
    "entry_impact_bps",
    "buy_count_5",
    "buy_count_15",
    "buy_count_30",
    "buy_count_60",
    "buy_count_120",
    "buy_count_300",
    "sell_count_5",
    "sell_count_15",
    "sell_count_30",
    "sell_count_60",
    "sell_count_120",
    "sell_count_300",
    "buy_quote_5",
    "buy_quote_15",
    "buy_quote_30",
    "buy_quote_60",
    "buy_quote_120",
    "buy_quote_300",
    "sell_quote_5",
    "sell_quote_15",
    "sell_quote_30",
    "sell_quote_60",
    "sell_quote_120",
    "sell_quote_300",
    "buy_sell_imbalance_30",
    "buy_sell_imbalance_60",
    "buy_sell_imbalance_300",
    "unique_buyers_15",
    "unique_buyers_30",
    "unique_buyers_60",
    "unique_buyers_300",
    "buyer_arrival_rate_5",
    "buyer_arrival_rate_15",
    "buyer_arrival_rate_30",
    "buyer_arrival_rate_60",
    "buyer_arrival_acceleration_15",
    "buyer_arrival_acceleration_30",
    "repeat_buyer_ratio_30",
    "repeat_buyer_ratio_60",
    "repeat_buyer_ratio_300",
    "buyer_retention_60",
    "buyer_retention_300",
    "wallet_churn_60",
    "wallet_churn_300",
    "buy_size_p10_30",
    "buy_size_p50_30",
    "buy_size_p90_30",
    "buy_size_p10_60",
    "buy_size_p50_60",
    "buy_size_p90_60",
    "buy_size_p10_300",
    "buy_size_p50_300",
    "buy_size_p90_300",
    "buy_size_cv_30",
    "buy_size_cv_60",
    "interarrival_cv_30",
    "interarrival_cv_60",
    "sell_acceleration_15",
    "sell_acceleration_30",
    "sell_acceleration_60",
    "largest_seller_share_30",
    "largest_seller_share_60",
    "largest_seller_share_300",
    "return_15",
    "return_30",
    "return_60",
    "return_300",
    "realized_volatility_30",
    "realized_volatility_60",
    "realized_volatility_300",
    "net_creator_flow_30",
    "net_creator_flow_60",
    "net_creator_flow_300",
];

#[derive(Clone, Debug)]
pub struct TradeTick {
    pub at: i64,
    pub side_buy: bool,
    pub base_atomic: u128,
    pub quote_atomic: u128,
    pub trader: String,
    pub price_ratio: Option<f64>,
}

#[derive(Default)]
pub struct WindowAggregator {
    ticks: VecDeque<TradeTick>,
    coverage_started_at: Option<i64>,
    first_seen_buyer: HashMap<String, i64>,
    inventory: HashMap<String, i128>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureWindowReadiness {
    WaitingNoCoverage,
    WaitingForHorizon {
        observed_seconds: u32,
        required_seconds: u32,
    },
    Ready,
}

impl WindowAggregator {
    pub fn push(&mut self, tick: TradeTick) {
        self.coverage_started_at = Some(
            self.coverage_started_at
                .map_or(tick.at, |started| started.min(tick.at)),
        );
        if tick.side_buy {
            self.first_seen_buyer
                .entry(tick.trader.clone())
                .or_insert(tick.at);
        }
        let signed_base = i128::try_from(tick.base_atomic).unwrap_or(i128::MAX);
        let inventory = self.inventory.entry(tick.trader.clone()).or_default();
        *inventory = if tick.side_buy {
            inventory.saturating_add(signed_base)
        } else {
            inventory.saturating_sub(signed_base)
        };
        self.ticks.push_back(tick);
        let cutoff = self.ticks.back().map(|t| t.at - 600).unwrap_or(0);
        while self.ticks.front().is_some_and(|t| t.at < cutoff) {
            self.ticks.pop_front();
        }
    }

    pub fn readiness(&self, now: i64, required_seconds: u32) -> FeatureWindowReadiness {
        let Some(started) = self.coverage_started_at else {
            return FeatureWindowReadiness::WaitingNoCoverage;
        };
        let observed_seconds = now.saturating_sub(started).clamp(0, i64::from(u32::MAX)) as u32;
        if observed_seconds < required_seconds {
            FeatureWindowReadiness::WaitingForHorizon {
                observed_seconds,
                required_seconds,
            }
        } else {
            FeatureWindowReadiness::Ready
        }
    }

    pub fn compute(
        &self,
        launch_id: &str,
        window_s: u32,
        as_of: ObservationPoint,
        age_seconds: Option<f64>,
        curve_progress: Option<f64>,
        buyers: usize,
        creator: Option<&str>,
    ) -> FeatureVector {
        let mut values = MARKET_FEATURE_NAMES
            .iter()
            .copied()
            .map(|name| (name.to_string(), Value::Null))
            .collect::<Map<_, _>>();
        values.insert("age_seconds".into(), opt(age_seconds));
        values.insert("curve_progress".into(), opt(curve_progress));
        values.insert("unique_buyers_lifetime".into(), json!(buyers as f64));
        let Some(now) = as_of
            .event_time
            .or(Some(as_of.observed_at))
            .map(|time| time.timestamp())
        else {
            return feature_vector(launch_id, window_s, as_of, values);
        };

        for window in OBSERVATION_WINDOWS {
            if !self.window_complete(now, window) {
                continue;
            }
            let rows = self.window(now, window);
            let buys = rows.iter().filter(|tick| tick.side_buy).count();
            let sells = rows.len().saturating_sub(buys);
            let buy_quote = quote_sum(rows.iter().copied().filter(|tick| tick.side_buy));
            let sell_quote = quote_sum(rows.iter().copied().filter(|tick| !tick.side_buy));
            values.insert(format!("buy_count_{window}"), json!(buys as f64));
            values.insert(format!("sell_count_{window}"), json!(sells as f64));
            values.insert(format!("buy_quote_{window}"), json!(buy_quote));
            values.insert(format!("sell_quote_{window}"), json!(sell_quote));
        }

        for window in [30, 60, 300] {
            let buy = number(&values, &format!("buy_quote_{window}"));
            let sell = number(&values, &format!("sell_quote_{window}"));
            values.insert(
                format!("buy_sell_imbalance_{window}"),
                buy.zip(sell)
                    .and_then(|(buy, sell)| {
                        ((buy + sell) > 0.0).then_some((buy - sell) / (buy + sell))
                    })
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            );
        }

        for window in [15, 30, 60, 300] {
            if self.window_complete(now, window) {
                let unique = self
                    .window(now, window)
                    .iter()
                    .filter(|tick| tick.side_buy)
                    .map(|tick| tick.trader.as_str())
                    .collect::<HashSet<_>>()
                    .len();
                values.insert(format!("unique_buyers_{window}"), json!(unique as f64));
            }
        }

        let full_history = self.full_history_complete(now, age_seconds);
        if full_history {
            for window in [5, 15, 30, 60] {
                if self.window_complete(now, window) {
                    let from = now - i64::from(window);
                    let arrivals = self
                        .first_seen_buyer
                        .values()
                        .filter(|seen| **seen > from && **seen <= now)
                        .count();
                    values.insert(
                        format!("buyer_arrival_rate_{window}"),
                        json!(arrivals as f64 / f64::from(window)),
                    );
                }
            }
            for window in [15, 30] {
                if self.window_complete(now, window * 2) {
                    let current = number(&values, &format!("buyer_arrival_rate_{window}"));
                    let previous_from = now - i64::from(window * 2);
                    let previous_to = now - i64::from(window);
                    let previous = self
                        .first_seen_buyer
                        .values()
                        .filter(|seen| **seen > previous_from && **seen <= previous_to)
                        .count() as f64
                        / f64::from(window);
                    values.insert(
                        format!("buyer_arrival_acceleration_{window}"),
                        current
                            .map(|current| Value::from(current - previous))
                            .unwrap_or(Value::Null),
                    );
                }
            }
        }

        for window in [30, 60, 300] {
            if !self.window_complete(now, window) {
                continue;
            }
            let mut counts = HashMap::<&str, usize>::new();
            for tick in self
                .window(now, window)
                .into_iter()
                .filter(|tick| tick.side_buy)
            {
                *counts.entry(&tick.trader).or_default() += 1;
            }
            if !counts.is_empty() {
                let repeats = counts.values().filter(|count| **count >= 2).count();
                values.insert(
                    format!("repeat_buyer_ratio_{window}"),
                    json!(repeats as f64 / counts.len() as f64),
                );
            }
        }

        if full_history {
            for window in [60, 300] {
                if !self.window_complete(now, window) {
                    continue;
                }
                let rows = self.window(now, window);
                let buyers = rows
                    .iter()
                    .filter(|tick| tick.side_buy)
                    .map(|tick| tick.trader.as_str())
                    .collect::<HashSet<_>>();
                if !buyers.is_empty() {
                    let retained = buyers
                        .iter()
                        .filter(|wallet| {
                            self.inventory.get(**wallet).copied().unwrap_or_default() > 0
                        })
                        .count();
                    values.insert(
                        format!("buyer_retention_{window}"),
                        json!(retained as f64 / buyers.len() as f64),
                    );
                }
                let mut at_start = self.inventory.clone();
                for tick in rows {
                    let amount = i128::try_from(tick.base_atomic).unwrap_or(i128::MAX);
                    let balance = at_start.entry(tick.trader.clone()).or_default();
                    *balance = if tick.side_buy {
                        balance.saturating_sub(amount)
                    } else {
                        balance.saturating_add(amount)
                    };
                }
                let holders = at_start
                    .iter()
                    .filter(|(_, amount)| **amount > 0)
                    .map(|(wallet, _)| wallet)
                    .collect::<Vec<_>>();
                if !holders.is_empty() {
                    let exited = holders
                        .iter()
                        .filter(|wallet| {
                            self.inventory.get(**wallet).copied().unwrap_or_default() <= 0
                        })
                        .count();
                    values.insert(
                        format!("wallet_churn_{window}"),
                        json!(exited as f64 / holders.len() as f64),
                    );
                }
            }
        }

        for window in [30, 60, 300] {
            if !self.window_complete(now, window) {
                continue;
            }
            let mut sizes = self
                .window(now, window)
                .into_iter()
                .filter(|tick| tick.side_buy)
                .map(|tick| tick.quote_atomic as f64)
                .collect::<Vec<_>>();
            sizes.sort_by(f64::total_cmp);
            values.insert(format!("buy_size_p10_{window}"), opt(quantile(&sizes, 0.1)));
            values.insert(format!("buy_size_p50_{window}"), opt(quantile(&sizes, 0.5)));
            values.insert(format!("buy_size_p90_{window}"), opt(quantile(&sizes, 0.9)));
            if window != 300 {
                values.insert(format!("buy_size_cv_{window}"), opt(cv(&sizes)));
                let times = self
                    .window(now, window)
                    .iter()
                    .map(|tick| tick.at as f64)
                    .collect::<Vec<_>>();
                let gaps = times
                    .windows(2)
                    .map(|pair| pair[1] - pair[0])
                    .collect::<Vec<_>>();
                values.insert(format!("interarrival_cv_{window}"), opt(cv(&gaps)));
            }
        }

        for window in [15, 30, 60] {
            if !self.window_complete(now, window * 2) {
                continue;
            }
            let current = quote_sum(
                self.window(now, window)
                    .into_iter()
                    .filter(|tick| !tick.side_buy),
            );
            let previous = quote_sum(
                self.interval(now - i64::from(window * 2), now - i64::from(window))
                    .into_iter()
                    .filter(|tick| !tick.side_buy),
            );
            values.insert(
                format!("sell_acceleration_{window}"),
                json!((current - previous) / f64::from(window)),
            );
        }

        for window in [30, 60, 300] {
            if !self.window_complete(now, window) {
                continue;
            }
            let sells = self
                .window(now, window)
                .into_iter()
                .filter(|tick| !tick.side_buy)
                .collect::<Vec<_>>();
            let total = quote_sum(sells.iter().copied());
            if total > 0.0 {
                let mut by_wallet = HashMap::<&str, f64>::new();
                for tick in sells {
                    *by_wallet.entry(&tick.trader).or_default() += tick.quote_atomic as f64;
                }
                let largest = by_wallet.values().copied().fold(0.0, f64::max);
                values.insert(
                    format!("largest_seller_share_{window}"),
                    json!(largest / total),
                );
            }
        }

        let current_price = self
            .ticks
            .iter()
            .rev()
            .find(|tick| tick.at <= now)
            .and_then(|tick| tick.price_ratio)
            .filter(|price| *price > 0.0);
        for window in [15, 30, 60, 300] {
            if !self.window_complete(now, window) {
                continue;
            }
            let past_price = self
                .ticks
                .iter()
                .rev()
                .find(|tick| tick.at <= now - i64::from(window))
                .and_then(|tick| tick.price_ratio)
                .filter(|price| *price > 0.0);
            values.insert(
                format!("return_{window}"),
                opt(current_price
                    .zip(past_price)
                    .map(|(current, past)| current / past - 1.0)),
            );
        }

        for window in [30, 60, 300] {
            if !self.window_complete(now, window) {
                continue;
            }
            let prices = self
                .window(now, window)
                .iter()
                .filter_map(|tick| tick.price_ratio)
                .filter(|price| *price > 0.0)
                .collect::<Vec<_>>();
            let volatility = (prices.len() >= 2).then(|| {
                prices
                    .windows(2)
                    .map(|pair| (pair[1] / pair[0]).ln().powi(2))
                    .sum::<f64>()
                    .sqrt()
            });
            values.insert(format!("realized_volatility_{window}"), opt(volatility));
        }

        if let Some(creator) = creator {
            for window in [30, 60, 300] {
                if !self.window_complete(now, window) {
                    continue;
                }
                let flow = self
                    .window(now, window)
                    .into_iter()
                    .filter(|tick| tick.trader == creator)
                    .map(|tick| {
                        if tick.side_buy {
                            tick.quote_atomic as f64
                        } else {
                            -(tick.quote_atomic as f64)
                        }
                    })
                    .sum::<f64>();
                values.insert(format!("net_creator_flow_{window}"), json!(flow));
            }
        }
        feature_vector(launch_id, window_s, as_of, values)
    }

    fn window_complete(&self, now: i64, window_s: u32) -> bool {
        self.readiness(now, window_s) == FeatureWindowReadiness::Ready
    }

    fn full_history_complete(&self, now: i64, age_seconds: Option<f64>) -> bool {
        age_seconds
            .filter(|age| age.is_finite() && *age >= 0.0)
            .zip(self.coverage_started_at)
            .is_some_and(|(age, started)| started <= now - age.floor() as i64 + 1)
    }

    fn window(&self, now: i64, window_s: u32) -> Vec<&TradeTick> {
        self.interval(now - i64::from(window_s), now)
    }

    fn interval(&self, after: i64, through: i64) -> Vec<&TradeTick> {
        self.ticks
            .iter()
            .filter(|tick| tick.at > after && tick.at <= through)
            .collect()
    }

    pub fn unique_buyers(&self) -> u64 {
        self.ticks
            .iter()
            .filter(|t| t.side_buy)
            .map(|t| t.trader.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len() as u64
    }

    pub fn imbalance(&self, window_s: i64) -> Option<f64> {
        let now = self.ticks.back().map(|t| t.at).unwrap_or(0);
        let from = now.saturating_sub(window_s);
        let w: Vec<&TradeTick> = self.ticks.iter().filter(|t| t.at >= from).collect();
        let buy_q: f64 = w
            .iter()
            .filter(|t| t.side_buy)
            .map(|t| t.quote_atomic as f64)
            .sum();
        let sell_q: f64 = w
            .iter()
            .filter(|t| !t.side_buy)
            .map(|t| t.quote_atomic as f64)
            .sum();
        let tot = buy_q + sell_q;
        if tot == 0.0 {
            None
        } else {
            Some((buy_q - sell_q) / tot)
        }
    }

    pub fn flow_quote(&self, window_s: i64) -> Option<u128> {
        let now = self.ticks.back().map(|t| t.at).unwrap_or(0);
        let from = now.saturating_sub(window_s);
        let sum = self
            .ticks
            .iter()
            .filter(|t| t.at >= from)
            .map(|t| t.quote_atomic)
            .fold(0u128, |a, b| a.saturating_add(b));
        if sum == 0 {
            None
        } else {
            Some(sum)
        }
    }

    pub fn return_over(&self, window_s: i64) -> Option<f64> {
        let now = self.ticks.back().map(|t| t.at).unwrap_or(0);
        let from = now.saturating_sub(window_s);
        let priced: Vec<f64> = self
            .ticks
            .iter()
            .filter(|t| t.at >= from)
            .filter_map(|t| t.price_ratio)
            .filter(|p| *p > 0.0)
            .collect();
        let first = priced.first().copied()?;
        let last = priced.last().copied()?;
        if first == 0.0 {
            None
        } else {
            Some((last / first) - 1.0)
        }
    }
}

fn feature_vector(
    launch_id: &str,
    window_s: u32,
    as_of: ObservationPoint,
    values: Map<String, Value>,
) -> FeatureVector {
    FeatureVector {
        feature_snapshot_id: uuid::Uuid::now_v7().to_string(),
        launch_id: launch_id.to_string(),
        observation_window_seconds: window_s,
        as_of,
        schema_version: "features-v1".into(),
        values,
        dependency_ids: vec![],
        quality: DataQuality::live(0, 0, true),
    }
}

fn quote_sum<'a>(ticks: impl Iterator<Item = &'a TradeTick>) -> f64 {
    ticks.map(|tick| tick.quote_atomic as f64).sum()
}

fn number(values: &Map<String, Value>, name: &str) -> Option<f64> {
    values.get(name).and_then(Value::as_f64)
}

fn quantile(sorted: &[f64], percentile: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let index = percentile * (sorted.len() - 1) as f64;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let weight = index - lower as f64;
    Some(sorted[lower] * (1.0 - weight) + sorted[upper] * weight)
}

fn cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean == 0.0 {
        return None;
    }
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    Some(variance.sqrt() / mean.abs())
}

fn opt(v: Option<f64>) -> Value {
    v.map(Value::from).unwrap_or(Value::Null)
}

pub fn heuristic_rank(age_s: f64, buyers: f64, imbalance: Option<f64>, curve: Option<f64>) -> f64 {
    let mut score = 0.0;
    if (30.0..900.0).contains(&age_s) {
        score += 20.0;
    }
    score += buyers.min(80.0);
    if let Some(i) = imbalance {
        if i > 0.2 {
            score += 15.0;
        }
    }
    if let Some(c) = curve {
        if (0.1..0.85).contains(&c) {
            score += 10.0;
        }
    }
    score.min(100.0)
}

#[allow(dead_code)]
fn _now() {
    let _ = now_utc();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumphound_domain::{Commitment, Slot};

    fn observation() -> ObservationPoint {
        let at = now_utc();
        ObservationPoint {
            slot: Slot(1),
            tx_index: None,
            event_index: None,
            event_time: Some(at),
            observed_at: at,
            knowledge_cutoff: at,
            commitment: Commitment::Confirmed,
        }
    }

    #[test]
    fn live_market_names_match_checked_in_schema() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../schemas/json/feature-schema-v1.json"))
                .unwrap();
        let ordered = schema["ordered_feature_names"].as_array().unwrap();
        let expected = ordered
            .iter()
            .take_while(|name| name.as_str() != Some("holder_top_1_share"))
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(MARKET_FEATURE_NAMES, expected);
    }

    #[test]
    fn emits_all_market_keys_and_nulls_incomplete_windows() {
        let mut aggregator = WindowAggregator::default();
        let as_of = observation();
        let now = as_of.event_time.unwrap().timestamp();
        aggregator.push(TradeTick {
            at: now,
            side_buy: true,
            base_atomic: 10,
            quote_atomic: 20,
            trader: "buyer".into(),
            price_ratio: Some(2.0),
        });
        let vector = aggregator.compute("launch", 60, as_of, Some(0.0), None, 1, Some("creator"));
        assert!(MARKET_FEATURE_NAMES
            .iter()
            .all(|name| vector.values.contains_key(*name)));
        assert_eq!(vector.values["buy_count_5"], Value::Null);
        assert_eq!(vector.values["buy_quote_5"], Value::Null);
    }

    #[test]
    fn computes_multi_window_flow_without_fabricated_missing_values() {
        let mut aggregator = WindowAggregator::default();
        let as_of = observation();
        let now = as_of.event_time.unwrap().timestamp();
        for offset in (0..=300).step_by(5) {
            aggregator.push(TradeTick {
                at: now - 300 + offset,
                side_buy: offset % 10 == 0,
                base_atomic: 10,
                quote_atomic: 20,
                trader: format!("wallet-{}", offset % 30),
                price_ratio: Some(1.0 + offset as f64 / 1_000.0),
            });
        }
        let vector = aggregator.compute(
            "launch",
            60,
            as_of,
            Some(300.0),
            Some(0.5),
            10,
            Some("wallet-0"),
        );
        for window in OBSERVATION_WINDOWS {
            assert!(vector.values[&format!("buy_count_{window}")].is_number());
            assert!(vector.values[&format!("sell_count_{window}")].is_number());
            assert!(vector.values[&format!("buy_quote_{window}")].is_number());
            assert!(vector.values[&format!("sell_quote_{window}")].is_number());
        }
        assert!(vector.values["realized_volatility_300"].is_number());
        assert!(vector.values["net_creator_flow_300"].is_number());
    }

    #[test]
    fn readiness_transitions_from_retryable_wait_to_ready() {
        let mut aggregator = WindowAggregator::default();
        assert_eq!(
            aggregator.readiness(100, 30),
            FeatureWindowReadiness::WaitingNoCoverage
        );
        aggregator.push(TradeTick {
            at: 100,
            side_buy: true,
            base_atomic: 1,
            quote_atomic: 1,
            trader: "buyer".into(),
            price_ratio: Some(1.0),
        });
        assert_eq!(
            aggregator.readiness(129, 30),
            FeatureWindowReadiness::WaitingForHorizon {
                observed_seconds: 29,
                required_seconds: 30
            }
        );
        assert_eq!(aggregator.readiness(130, 30), FeatureWindowReadiness::Ready);
    }
}
