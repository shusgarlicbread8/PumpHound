//! Tail-concentration metrics for heavy-tailed paper books. Pure integer accounting over closed
//! per-trade P&L (lamports). These exist so a single outlier can never hide inside a headline.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TailMetrics {
    pub n: usize,
    pub net_pnl_atomic: i128,
    pub gross_profit_atomic: i128,
    pub gross_loss_atomic: i128,
    /// Net P&L with the single largest winner removed.
    pub net_ex_largest_winner_atomic: i128,
    /// Net P&L with the five largest winners removed.
    pub net_ex_top5_winners_atomic: i128,
    /// Net P&L with the top 1% of winners (at least one trade) removed.
    pub net_ex_top_1pct_winners_atomic: i128,
    /// Sum of the worst 50% of trades.
    pub net_bottom_50pct_atomic: i128,
    /// Sum of the best 10% of trades (at least one).
    pub net_top_10pct_atomic: i128,
    /// Share of gross profit contributed by the top-k winners. `None` when there is no profit.
    pub top1_share_of_profit: Option<f64>,
    pub top5_share_of_profit: Option<f64>,
    pub top10_share_of_profit: Option<f64>,
    pub top_1pct_share_of_profit: Option<f64>,
    /// gross_profit / |gross_loss|. `None` when there are no losses.
    pub profit_factor: Option<f64>,
    pub mean_pnl_atomic: Option<f64>,
    pub median_pnl_atomic: Option<f64>,
    /// sqrt(mean(min(pnl,0)^2)).
    pub downside_deviation_atomic: Option<f64>,
    /// mean / downside deviation. `None` when downside deviation is zero.
    pub sortino_like: Option<f64>,
    /// Sign flips when the largest winner is removed: the headline depends on one trade.
    pub headline_depends_on_largest_winner: bool,
}

fn ceil_pct(n: usize, pct: usize) -> usize {
    ((n * pct) + 99) / 100
}

pub fn tail_metrics(closed_pnls: &[i128]) -> TailMetrics {
    let n = closed_pnls.len();
    if n == 0 {
        return TailMetrics::default();
    }
    let mut sorted = closed_pnls.to_vec();
    sorted.sort_unstable();
    let net: i128 = sorted.iter().sum();
    let gross_profit: i128 = sorted.iter().filter(|p| **p > 0).sum();
    let gross_loss: i128 = sorted.iter().filter(|p| **p < 0).sum();
    let winners_desc: Vec<i128> = sorted.iter().rev().copied().filter(|p| *p > 0).collect();
    let top_k = |k: usize| -> i128 { winners_desc.iter().take(k).sum() };
    let top_1pct_k = ceil_pct(n, 1).max(1);
    let share = |k: usize| (gross_profit > 0).then(|| top_k(k) as f64 / gross_profit as f64);
    let bottom_50 = n / 2;
    let net_bottom_50pct: i128 = sorted.iter().take(bottom_50).sum();
    let top_10_k = ceil_pct(n, 10).max(1);
    let net_top_10pct: i128 = sorted.iter().rev().take(top_10_k).sum();
    let mean = net as f64 / n as f64;
    let median = if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] as f64 + sorted[n / 2] as f64) / 2.0
    };
    let downside_sq: f64 = sorted
        .iter()
        .map(|p| (*p).min(0) as f64)
        .map(|d| d * d)
        .sum::<f64>()
        / n as f64;
    let downside_dev = downside_sq.sqrt();
    let ex_largest = net - top_k(1);
    TailMetrics {
        n,
        net_pnl_atomic: net,
        gross_profit_atomic: gross_profit,
        gross_loss_atomic: gross_loss,
        net_ex_largest_winner_atomic: ex_largest,
        net_ex_top5_winners_atomic: net - top_k(5),
        net_ex_top_1pct_winners_atomic: net - top_k(top_1pct_k),
        net_bottom_50pct_atomic: net_bottom_50pct,
        net_top_10pct_atomic: net_top_10pct,
        top1_share_of_profit: share(1),
        top5_share_of_profit: share(5),
        top10_share_of_profit: share(10),
        top_1pct_share_of_profit: share(top_1pct_k),
        profit_factor: (gross_loss < 0).then(|| gross_profit as f64 / (-gross_loss) as f64),
        mean_pnl_atomic: Some(mean),
        median_pnl_atomic: Some(median),
        downside_deviation_atomic: Some(downside_dev),
        sortino_like: (downside_dev > 0.0).then(|| mean / downside_dev),
        headline_depends_on_largest_winner: (net > 0) != (ex_largest > 0) && !winners_desc.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_default() {
        assert_eq!(tail_metrics(&[]), TailMetrics::default());
    }

    #[test]
    fn one_outlier_dominates_headline() {
        // Nine -0.1 SOL losers and one +5.5 SOL winner: the exact PumpHound headline shape.
        let mut pnls = vec![-100_000_000i128; 9];
        pnls.push(5_500_000_000);
        let t = tail_metrics(&pnls);
        assert_eq!(t.net_pnl_atomic, 4_600_000_000);
        assert_eq!(t.net_ex_largest_winner_atomic, -900_000_000);
        assert_eq!(t.net_ex_top_1pct_winners_atomic, -900_000_000);
        assert_eq!(t.top1_share_of_profit, Some(1.0));
        assert!(t.headline_depends_on_largest_winner);
        assert_eq!(t.net_bottom_50pct_atomic, -500_000_000);
        assert!((t.profit_factor.unwrap() - 5_500.0 / 900.0).abs() < 1e-9);
    }

    #[test]
    fn top_1pct_is_at_least_one_trade_and_scales() {
        let pnls: Vec<i128> = (1..=200).map(|i| i as i128 * 1_000).collect();
        let t = tail_metrics(&pnls);
        // ceil(1% of 200) = 2 winners removed: 200k + 199k
        assert_eq!(
            t.net_ex_top_1pct_winners_atomic,
            t.net_pnl_atomic - 399_000
        );
        assert_eq!(t.net_ex_top5_winners_atomic, t.net_pnl_atomic - (200 + 199 + 198 + 197 + 196) * 1_000);
        assert_eq!(t.profit_factor, None);
        assert!(!t.headline_depends_on_largest_winner);
    }

    #[test]
    fn diversified_book_does_not_depend_on_one_trade() {
        let pnls = vec![300i128, 200, 250, -100, 400, -50, 350];
        let t = tail_metrics(&pnls);
        assert!(!t.headline_depends_on_largest_winner);
        assert!(t.top1_share_of_profit.unwrap() < 0.3);
        assert_eq!(t.median_pnl_atomic, Some(250.0));
    }
}
