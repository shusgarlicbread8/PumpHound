//! Read-only book-level ledger projections. Historical policies are never modified.
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_losses_are_drawdowns_from_zero_capital_pnl() {
        assert_eq!(max_drawdown(&[-5, -3, 12, -2]), 8);
        assert_eq!(max_drawdown(&[-5]), 5);
    }
    #[test]
    fn sample_sufficiency_does_not_validate_edge() {
        assert_eq!(sample_status(145), "SUFFICIENT");
        assert_eq!(edge_status(145, -20), "NEGATIVE");
        assert_eq!(edge_status(145, 20), "PROMISING");
        assert_eq!(edge_status(2, 20), "INCONCLUSIVE");
    }
    #[test]
    fn financial_units_preserve_signed_atomic_precision() {
        let v = sol_money(-1);
        assert_eq!(v["atomic"], "-1");
        assert_eq!(v["value"], "-0.000000001");
        assert_eq!(v["decimals"], 9);
        assert_eq!(sol_money(1_250_000_000)["value"], "1.250000000");
    }
}

use chrono::{DateTime, Utc};
use pumphound_domain::WSOL_MINT;
use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

fn max_drawdown(pnls: &[i128]) -> i128 {
    let (mut equity, mut peak, mut maximum) = (0, 0, 0);
    for pnl in pnls {
        equity += pnl;
        peak = peak.max(equity);
        maximum = maximum.max(peak - equity);
    }
    maximum
}
fn sample_status(n: usize) -> &'static str {
    if n < 30 {
        "INSUFFICIENT"
    } else if n < 100 {
        "EARLY"
    } else {
        "SUFFICIENT"
    }
}
fn edge_status(n: usize, net: i128) -> &'static str {
    if n >= 30 && net < 0 {
        "NEGATIVE"
    } else if n >= 30 && net > 0 {
        "PROMISING"
    } else {
        "INCONCLUSIVE"
    }
}
pub fn sol_money(atomic: i128) -> Value {
    let abs = atomic.unsigned_abs();
    let value = format!(
        "{}{}.{:09}",
        if atomic < 0 { "-" } else { "" },
        abs / 1_000_000_000,
        abs % 1_000_000_000
    );
    json!({"atomic":atomic.to_string(),"value":value,"display":format!("{value} SOL"),
        "symbol":"SOL","asset_mint":WSOL_MINT,"decimals":9,"status":"KNOWN"})
}
fn atomic(row: &sqlx::postgres::PgRow, column: &str) -> anyhow::Result<i128> {
    Ok(row.try_get::<String, _>(column)?.parse()?)
}
fn sol(atomic: i128) -> f64 {
    atomic as f64 / 1e9
}
fn median(values: &[i128]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (sol(values[mid - 1]) + sol(values[mid])) / 2.0
    } else {
        sol(values[mid])
    })
}
// Native SOL's persisted sentinel and WSOL denote the same quote unit here. This
// normalization is exposed in the audit, while unrecognized assets remain unknown.
fn quote_fees(components: &Value) -> Option<i128> {
    let mut sum = 0i128;
    for fee in components.as_array()? {
        let mint = fee.pointer("/asset/mint")?.as_str()?;
        if mint != WSOL_MINT && mint != "11111111111111111111111111111111" {
            return None;
        }
        if fee.pointer("/asset/decimals")?.as_u64()? != 9 {
            return None;
        }
        sum += fee
            .pointer("/asset/atomic")?
            .as_str()?
            .parse::<i128>()
            .ok()?;
    }
    Some(sum)
}

pub async fn strategy_reports(stores: &Stores) -> anyhow::Result<Value> {
    // One row per durable book. A strategy can exist in multiple experiments.
    let books = sqlx::query(
        r#"SELECT r.simulation_run_id,r.portfolio_id,r.manifest,r.created_at
        FROM pumphound.simulation_runs r
        JOIN pumphound.portfolios pf ON pf.portfolio_id=r.portfolio_id
        WHERE r.state='running' AND pf.archived_at IS NULL AND r.manifest ? 'strategy'
        ORDER BY r.created_at DESC,r.simulation_run_id"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut items = Vec::new();
    for book in books {
        let id: Uuid = book.get("simulation_run_id");
        let manifest: Value = book.get("manifest");
        let strategy = manifest["strategy"].as_str().unwrap_or("UNKNOWN");
        let positions = sqlx::query(
            r#"SELECT position_id::text,quote_mint,status,base_atomic::text,
          realized_pnl_quote_atomic::text,cost_quote_atomic::text,opened_at,closed_at
          FROM pumphound.paper_positions p WHERE simulation_run_id=$1
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
            )
          )
          ORDER BY closed_at NULLS LAST,position_id"#,
        )
        .bind(id)
        .fetch_all(&stores.postgres)
        .await?;
        let (mut net, mut exposure, mut open, mut non_sol) = (0i128, 0i128, 0usize, 0usize);
        for p in &positions {
            if matches!(p.get::<String, _>("status").as_str(), "OPEN" | "UNMARKABLE")
                && atomic(p, "base_atomic")? > 0
            {
                open += 1;
            }
            if p.get::<String, _>("quote_mint") != WSOL_MINT {
                non_sol += 1;
                continue;
            }
            let pnl = atomic(p, "realized_pnl_quote_atomic")?;
            net += pnl;
            exposure += atomic(p, "cost_quote_atomic")?;
        }
        let closed_lots = sqlx::query(
            r#"SELECT l.lot_id::text,
              SUM(c.realized_pnl_atomic)::text AS realized_pnl_atomic,
              EXTRACT(EPOCH FROM(MAX(sf.arrival_at)-l.acquired_at))::double precision AS hold_seconds
            FROM pumphound.acquisition_lots l
            JOIN pumphound.paper_fills bf ON bf.fill_id=l.fill_id
            JOIN pumphound.paper_orders bo ON bo.order_id=bf.order_id
            JOIN pumphound.decisions bd ON bd.decision_id=bo.decision_id
            JOIN pumphound.paper_positions pos
              ON pos.simulation_run_id=l.simulation_run_id
             AND pos.base_mint=l.base_mint
            JOIN pumphound.lot_consumptions c ON c.lot_id=l.lot_id
            JOIN pumphound.paper_fills sf ON sf.fill_id=c.sell_fill_id
            WHERE l.simulation_run_id=$1 AND l.quote_mint=$2
              AND NOT EXISTS (
                SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                WHERE iq.status='ACTIVE' AND (
                  (iq.entity_type='LOT' AND iq.entity_id=l.lot_id::text)
                  OR (iq.entity_type='LAUNCH' AND iq.entity_id=bd.launch_id::text)
                  OR (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
                  OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
                )
              )
            GROUP BY l.lot_id,l.base_atomic,l.acquired_at
            HAVING SUM(c.base_atomic)=l.base_atomic
            ORDER BY MAX(sf.arrival_at),l.lot_id"#,
        )
        .bind(id)
        .bind(WSOL_MINT)
        .fetch_all(&stores.postgres)
        .await?;
        let closed_pnls = closed_lots
            .iter()
            .map(|lot| atomic(lot, "realized_pnl_atomic"))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let holds = closed_lots
            .iter()
            .filter_map(|lot| lot.get::<Option<f64>, _>("hold_seconds"))
            .collect::<Vec<_>>();
        let closed = closed_lots.len();
        let closed_net: i128 = closed_pnls.iter().sum();
        let winners: Vec<i128> = closed_pnls.iter().copied().filter(|p| *p > 0).collect();
        let losers: Vec<i128> = closed_pnls.iter().copied().filter(|p| *p < 0).collect();
        let largest_win = winners.iter().max().copied();
        let largest_loss = losers.iter().min().copied();
        let drawdown = max_drawdown(&closed_pnls);
        let orders = sqlx::query(r#"SELECT COUNT(*)::bigint AS orders,
          COUNT(*) FILTER(WHERE EXISTS(SELECT 1 FROM pumphound.paper_fills f WHERE f.order_id=o.order_id AND f.simulation_run_id=o.simulation_run_id))::bigint AS filled,
          COUNT(*) FILTER(WHERE status='REJECTED')::bigint AS rejected
          FROM pumphound.paper_orders o
          JOIN pumphound.decisions od ON od.decision_id=o.decision_id
          WHERE o.simulation_run_id=$1 AND o.side='buy'
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=od.launch_id::text)
              OR (iq.entity_type='POSITION' AND EXISTS (
                SELECT 1 FROM pumphound.paper_positions pos
                WHERE pos.position_id::text=iq.entity_id
                  AND pos.simulation_run_id=o.simulation_run_id
                  AND pos.base_mint=o.output_mint
              ))
            )
          )"#)
          .bind(id).fetch_one(&stores.postgres).await?;
        let entries = sqlx::query_scalar::<_, String>(
            r#"SELECT f.input_atomic::text FROM pumphound.paper_fills f
          JOIN pumphound.paper_orders o ON o.order_id=f.order_id
          JOIN pumphound.decisions d ON d.decision_id=o.decision_id
          WHERE f.simulation_run_id=$1 AND o.side='buy' AND f.input_mint=$2
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
              OR (iq.entity_type='POSITION' AND EXISTS (
                SELECT 1 FROM pumphound.paper_positions pos
                WHERE pos.position_id::text=iq.entity_id
                  AND pos.simulation_run_id=o.simulation_run_id
                  AND pos.base_mint=f.output_mint
              ))
            )
          )"#,
        )
        .bind(id)
        .bind(WSOL_MINT)
        .fetch_all(&stores.postgres)
        .await?;
        let entries = entries
            .iter()
            .map(|v| v.parse::<i128>())
            .collect::<Result<Vec<_>, _>>()?;
        let decisions = sqlx::query(
            r#"SELECT COUNT(*)::bigint AS candidates,
          COUNT(*) FILTER(WHERE action='BUY')::bigint AS buys,
          COUNT(*) FILTER(WHERE action='WATCH')::bigint AS watches,
          COUNT(*) FILTER(WHERE action='ABSTAIN')::bigint AS abstains,
          COUNT(*) FILTER(WHERE action='AVOID')::bigint AS avoids,
          COALESCE(jsonb_agg(DISTINCT policy_version)
            FILTER (WHERE policy_version IS NOT NULL),'[]') AS policy_versions
          FROM pumphound.decisions d WHERE simulation_run_id=$1 AND decision_kind='ENTRY'
          AND COALESCE(heuristic_version,'') NOT IN ('ML_SHADOW','SHARED_ELIGIBLE_V1')
          AND decision_key NOT LIKE 'shadow:%' AND decision_key NOT LIKE 'universe:%'
          AND decision_key NOT LIKE 'book-intent:%'
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text AND iq.status='ACTIVE'
          )"#,
        )
        .bind(id)
        .fetch_one(&stores.postgres)
        .await?;
        // Allocate entry fees by FIFO cost share and exit fees by proceeds share.
        // Fee deductions already enter buy cost / net sell proceeds; never subtract twice.
        let consumptions = sqlx::query(
            r#"SELECT c.allocated_cost_atomic::text,c.net_proceeds_atomic::text,
          c.realized_pnl_atomic::text,l.cost_atomic::text,sf.output_atomic::text,
          bf.fee_components AS buy_fees,sf.fee_components AS sell_fees
          FROM pumphound.lot_consumptions c JOIN pumphound.acquisition_lots l ON l.lot_id=c.lot_id
          JOIN pumphound.paper_fills bf ON bf.fill_id=l.fill_id
          JOIN pumphound.paper_orders bo ON bo.order_id=bf.order_id
          JOIN pumphound.decisions bd ON bd.decision_id=bo.decision_id
          JOIN pumphound.paper_positions pos
            ON pos.simulation_run_id=l.simulation_run_id
           AND pos.base_mint=l.base_mint
          JOIN pumphound.paper_fills sf ON sf.fill_id=c.sell_fill_id
          WHERE l.simulation_run_id=$1 AND l.quote_mint=$2
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='LOT' AND iq.entity_id=l.lot_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=bd.launch_id::text)
              OR (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
            )
          )"#,
        )
        .bind(id)
        .bind(WSOL_MINT)
        .fetch_all(&stores.postgres)
        .await?;
        let (mut fees, mut lot_net, mut fees_known) = (0i128, 0i128, true);
        for c in consumptions {
            lot_net += atomic(&c, "realized_pnl_atomic")?;
            match (
                quote_fees(&c.get::<Value, _>("buy_fees")),
                quote_fees(&c.get::<Value, _>("sell_fees")),
            ) {
                (Some(buy), Some(sell)) => {
                    let cost = atomic(&c, "cost_atomic")?;
                    let output = atomic(&c, "output_atomic")?;
                    if cost > 0 && output > 0 {
                        fees += buy * atomic(&c, "allocated_cost_atomic")? / cost
                            + sell * atomic(&c, "net_proceeds_atomic")? / output;
                    } else {
                        fees_known = false;
                    }
                }
                _ => fees_known = false,
            }
        }
        let orders_n: i64 = orders.get("orders");
        let filled: i64 = orders.get("filled");
        let rejected: i64 = orders.get("rejected");
        let policy_versions: Value = decisions.get("policy_versions");
        let policy_version = manifest.get("policy_version").cloned().or_else(|| {
            policy_versions
                .as_array()
                .filter(|versions| versions.len() == 1)
                .and_then(|versions| versions.first().cloned())
        });
        let policy_version_state = if policy_version.is_some() {
            "KNOWN"
        } else if policy_versions
            .as_array()
            .is_some_and(|versions| versions.len() > 1)
        {
            "MULTIPLE_PER_DECISION"
        } else {
            "UNKNOWN"
        };
        let authority = if strategy.contains("CONTROL_FLOW") || strategy.contains("RANDOM_CONTROL")
        {
            "CONTROL"
        } else if strategy.contains("SHADOW") {
            "SHADOW"
        } else {
            "PAPER_ACTIVE"
        };
        let caveats = vec![
            "Observed paper results do not establish alpha or out-of-sample significance.",
            "Book comparisons differ in sizing, capacity, selection and exit policy; no incremental-lift claim.",
            "Protocol/creator fees are embedded in fill prices. Network/priority fees are not modeled; slippage/impact are embedded, not separately attributable.",
            "Native SOL fee sentinel is normalized to SOL; historical schedule label says 125bps but quote implementation applies 95+100bps. Historical records are unchanged.",
            "Fee allocation uses FIFO realized-cost/proceeds shares, rounded down per allocation. Gross excludes modeled protocol/creator fees only.",
            "Closed N and expectancy use fully FIFO-consumed acquisition lots (one entry fill lot per realized trade). Open positions remain aggregate book/mint holdings.",
            "MFE/MAE and unrealized mark P&L are unknown without a complete historical executable mark series. Exposure is remaining cost basis.",
        ];
        let report = json!({
          "status":sample_status(closed),"sample_status":sample_status(closed),"edge_status":edge_status(closed_pnls.len(),closed_net),"authority":authority,
          "criteria":{"sample":"INSUFFICIENT <30; EARLY 30–99; SUFFICIENT >=100 closed positions","edge":"NEGATIVE/PROMISING require >=30 SOL closed positions and negative/positive net; otherwise INCONCLUSIVE. POSITIVE_OOS requires independent evaluation and is never inferred here."},
          "candidates":decisions.get::<i64,_>("candidates"),"buys":decisions.get::<i64,_>("buys"),"watches":decisions.get::<i64,_>("watches"),"abstains":decisions.get::<i64,_>("abstains"),"avoids":decisions.get::<i64,_>("avoids"),
          "trades":orders_n,"fills":filled,"fill_rate":(orders_n>0).then_some(filled as f64/orders_n as f64),"reject_rate":(orders_n>0).then_some(rejected as f64/orders_n as f64),
          "open_positions":open,"closed_trades":closed,"net_pnl":sol(net),"net_pnl_quote_atomic":net.to_string(),"closed_net_pnl":sol(closed_net),
          "gross_pnl":fees_known.then_some(sol(net+fees)),"fees":fees_known.then_some(sol(fees)),
          "expectancy":(!closed_pnls.is_empty()).then_some(sol(closed_net)/closed_pnls.len().max(1) as f64),
          "win_rate":(!closed_pnls.is_empty()).then_some(winners.len() as f64/closed_pnls.len().max(1) as f64),
          "max_drawdown":sol(drawdown),"exposure":sol(exposure),"capital":"SIMULATED",
          "mean_entry_size":(!entries.is_empty()).then_some(sol(entries.iter().sum())/entries.len().max(1) as f64),"median_entry_size":median(&entries),
          "median_winner":median(&winners),"median_loser":median(&losers),"largest_winner":largest_win.map(sol),"largest_loser":largest_loss.map(sol),
          "net_without_largest_winner":largest_win.map(|w|sol(closed_net-w)),"largest_winner_share_of_positive_pnl":largest_win.map(|w|w as f64/winners.iter().sum::<i128>().max(1) as f64),
          "avg_hold":(!holds.is_empty()).then_some(holds.iter().sum::<f64>()/holds.len().max(1) as f64),"hold_unit":"seconds",
          "mfe":null,"mae":null,"mark_status":"UNKNOWN","sharpe_like":null,"regime":null,
          "financial_units":{"quote_currency":"SOL","decimals":9,"metric_values":"SOL","atomic_values":"lamports","asset_mint":WSOL_MINT},
          "closed_trade_basis":"FULLY_FIFO_CONSUMED_ENTRY_LOT",
          "money":{"net_pnl":sol_money(net),"gross_pnl":fees_known.then(||sol_money(net+fees)),"fees":fees_known.then(||sol_money(fees)),"max_drawdown":sol_money(drawdown),"exposure":sol_money(exposure)},
          "attribution":{"position_net_quote_atomic":net.to_string(),"fifo_net_quote_atomic":lot_net.to_string(),"reconciled":net==lot_net,"difference_quote_atomic":(net-lot_net).to_string(),"fee_status":if fees_known {"ALLOCATED_MODELED_FEES"}else{"UNKNOWN_ASSET_OR_COMPONENT"},"excluded_non_sol_positions":non_sol},
          "position_sizing":manifest.get("position_sizing"),"fee_model_version":manifest.get("fee_model_version"),"audit_caveats":caveats,
          "sample_warning":(closed<100).then(||format!("{}; N={closed}",sample_status(closed))),"experiment_id":manifest.get("experiment_id"),"fabricated":false
        });
        items.push(json!({"report_id":format!("ledger-{id}"),"simulation_run_id":id,"book_id":id,
          "portfolio_id":book.get::<Uuid,_>("portfolio_id"),"strategy_key":strategy,
          "strategy_version":manifest.get("strategy_version"),"experiment_id":manifest.get("experiment_id"),
          "policy_version":policy_version,"policy_versions":policy_versions,
          "policy_version_state":policy_version_state,
          "execution_envelope_policy":manifest.get("execution_envelope_policy"),
          "model_versions":null,"model_versions_status":"PER_DECISION_SEE_TRACE",
          "generated_at":Utc::now(),"report_sha256":pumphound_domain::sha256_hex(report.to_string().as_bytes()),
          "period_start":positions.iter().map(|p|p.get::<DateTime<Utc>,_>("opened_at")).min(),
          "period_end":positions.iter().filter_map(|p|p.get::<Option<DateTime<Utc>>,_>("closed_at")).max(),
          "closed_trades":closed,"open_positions":open,"trades":orders_n,"net_pnl":report["net_pnl"],"expectancy":report["expectancy"],"max_drawdown":report["max_drawdown"],"win_rate":report["win_rate"],"status":report["status"],
          "sample_status":report["sample_status"],"edge_status":report["edge_status"],"authority":authority,"financial_units":report["financial_units"],"report":report}));
    }
    Ok(
        json!({"items":items,"scope":{"books":"ALL_ACTIVE_BOOKS","window":"ALL_TIME","active_definition":"running simulation run; unarchived portfolio; strategy in manifest","closed_n":"fully FIFO-consumed SOL acquisition lots; one entry fill lot per realized trade","open_positions":"aggregate book/mint holdings","net_pnl":"realized SOL position P&L including partial exits"},"as_of":Utc::now(),"fabricated":false}),
    )
}
