//! Read-only product contract over durable paper evidence. No synthetic history.
use chrono::{DateTime, Utc};
use pumphound_storage::Stores;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Default, Deserialize)]
pub struct TruthQuery {
    pub book_id: Option<Uuid>,
    pub experiment_id: Option<String>,
    pub strategy: Option<String>,
    pub state: Option<String>,
    pub window: Option<String>,
    /// Home and other KPI surfaces need the same book/window/P&L contract
    /// without materializing the full position/order/activity projections.
    #[serde(default)]
    pub summary_only: bool,
}

#[derive(Default, Deserialize)]
pub struct TraceQuery {
    pub at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporal_evidence_uses_persistence_time_too() {
        let cutoff = DateTime::parse_from_rfc3339("2026-09-08T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut events = vec![
            json!({"at":"2026-09-08T09:00:00Z","known_at":"2026-09-08T11:00:00Z"}),
            json!({"at":"2026-09-08T09:00:00Z","known_at":"2026-09-08T09:00:00Z"}),
        ];
        apply_time(&mut events, Some(cutoff), Some(cutoff));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["knowledge"], "KNOWN_AT_DECISION_TIME");
    }
    #[test]
    fn future_outcome_is_never_available_at_entry() {
        let entry = DateTime::parse_from_rfc3339("2026-09-08T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut events = vec![
            json!({"at":"2026-09-08T11:00:00Z","known_at":"2026-09-08T11:00:00Z","kind":"POSITION_CLOSED","payload":{"pnl":100}}),
        ];
        apply_time(&mut events, Some(entry), Some(entry));
        assert!(events.is_empty());
    }
    #[test]
    fn missing_time_never_counts_as_known() {
        let mut events = vec![json!({"at":null,"known_at":null})];
        apply_time(&mut events, Some(Utc::now()), Some(Utc::now()));
        assert!(events.is_empty());
    }
    #[test]
    fn scope_rejects_ambiguous_filters() {
        assert!(validate(&TruthQuery {
            state: Some("nonsense".into()),
            ..Default::default()
        })
        .is_err());
        assert!(validate(&TruthQuery {
            window: Some("yesterday".into()),
            ..Default::default()
        })
        .is_err());
        assert!(validate(&TruthQuery::default()).is_ok());
    }
}

pub fn validate(q: &TruthQuery) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(q.window.as_deref(), None | Some("all" | "today")),
        "window must be all or today (UTC)"
    );
    anyhow::ensure!(
        q.state.as_deref().is_none_or(|state| matches!(
            state.to_ascii_uppercase().as_str(),
            "ALL" | "OPEN" | "CLOSED" | "UNMARKABLE"
        )),
        "state must be ALL, OPEN, CLOSED, or UNMARKABLE"
    );
    Ok(())
}
fn canonical_state(state: Option<&str>) -> Option<String> {
    state
        .map(str::to_ascii_uppercase)
        .filter(|state| state != "ALL")
}
fn event_time(e: &Value, key: &str) -> Option<DateTime<Utc>> {
    e[key]
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|x| x.with_timezone(&Utc))
}
fn apply_time(
    events: &mut Vec<Value>,
    at: Option<DateTime<Utc>>,
    decision_at: Option<DateTime<Utc>>,
) {
    events.retain(|e| {
        at.is_none_or(|v| {
            event_time(e, "known_at").is_some_and(|t| t <= v)
                && event_time(e, "at").is_some_and(|t| t <= v)
        })
    });
    for event in events.iter_mut() {
        event["knowledge"] = json!(if event_time(event, "known_at")
            .zip(decision_at)
            .is_some_and(|(t, d)| t <= d)
        {
            "KNOWN_AT_DECISION_TIME"
        } else {
            "OBSERVED_LATER"
        });
    }
    events.sort_by(|a, b| {
        event_time(a, "known_at")
            .cmp(&event_time(b, "known_at"))
            .then_with(|| a["event_id"].as_str().cmp(&b["event_id"].as_str()))
    });
}

pub async fn report(stores: &Stores, q: &TruthQuery) -> anyhow::Result<Value> {
    validate(q)?;
    let mut tx = stores.postgres.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let all_books: Value=sqlx::query_scalar(r#"SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'book_id',r.simulation_run_id,'portfolio_id',r.portfolio_id,'experiment_id',r.manifest->>'experiment_id',
      'strategy_version',r.manifest->>'strategy',
      'policy_version',COALESCE(r.manifest->>'policy_version',policies.single_policy_version),
      'policy_versions',policies.policy_versions,
      'policy_version_state',CASE
        WHEN COALESCE(r.manifest->>'policy_version',policies.single_policy_version) IS NOT NULL THEN 'KNOWN'
        WHEN jsonb_array_length(policies.policy_versions)>1 THEN 'MULTIPLE_PER_DECISION'
        ELSE 'UNKNOWN'
      END,
      'name',p.name,'manifest',r.manifest) ORDER BY r.created_at),'[]')
      FROM pumphound.simulation_runs r JOIN pumphound.portfolios p USING(portfolio_id)
      LEFT JOIN LATERAL (
        SELECT
          CASE WHEN COUNT(DISTINCT d.policy_version)=1 THEN MIN(d.policy_version) END single_policy_version,
          COALESCE(jsonb_agg(DISTINCT d.policy_version) FILTER (WHERE d.policy_version IS NOT NULL),'[]') policy_versions
        FROM pumphound.decisions d WHERE d.simulation_run_id=r.simulation_run_id
      ) policies ON true
      WHERE r.state='running' AND p.archived_at IS NULL AND r.manifest ? 'strategy'"#).fetch_one(&mut *tx).await?;
    let selected_books = all_books
        .as_array()
        .into_iter()
        .flatten()
        .filter(|book| {
            q.book_id.is_none_or(|id| {
                book["book_id"]
                    .as_str()
                    .and_then(|value| Uuid::parse_str(value).ok())
                    == Some(id)
            }) && q
                .experiment_id
                .as_ref()
                .is_none_or(|value| book["experiment_id"].as_str() == Some(value))
                && q.strategy
                    .as_ref()
                    .is_none_or(|value| book["strategy_version"].as_str() == Some(value))
        })
        .cloned()
        .collect::<Vec<_>>();
    let ids: Vec<Uuid> = selected_books
        .iter()
        .filter_map(|book| Uuid::parse_str(book["book_id"].as_str()?).ok())
        .collect();
    let books = Value::Array(selected_books);
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let since = if q.window.as_deref() == Some("today") {
        Some(now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc())
    } else {
        None
    };
    let state = canonical_state(q.state.as_deref());
    let empty = json!([]);
    let positions:Value=if q.summary_only { empty.clone() } else { sqlx::query_scalar(r#"SELECT COALESCE(jsonb_agg(v ORDER BY v->>'opened_at' DESC),'[]') FROM (
      SELECT to_jsonb(pos)||jsonb_build_object('trade_trace_id',pos.position_id,'book_id',pos.simulation_run_id,
       'experiment_id',r.manifest->>'experiment_id','strategy_version',r.manifest->>'strategy',
       'policy_version',COALESCE(entry.policy_version,r.manifest->>'policy_version'),
       'base_atomic',pos.base_atomic::text,'cost_quote_atomic',pos.cost_quote_atomic::text,
       'realized_pnl_quote_atomic',pos.realized_pnl_quote_atomic::text,'base_decimals',a.decimals,
       'quote_decimals',qa.decimals,'symbol',a.symbol,'entry',entry.v,
       'entry_decision_id',entry.decision_id,'entry_order_id',entry.order_id,
       'entry_fill_id',entry.fill_id,'feature_snapshot_id',entry.feature_snapshot_id,
       'model_versions',entry.model_versions,'mark',mark.v,
       'integrity_state',CASE WHEN iq.quarantine_id IS NULL THEN 'VERIFIED' ELSE 'QUARANTINED' END,
       'integrity_reason',iq.reason_code,'integrity_evidence',iq.evidence) v
      FROM pumphound.paper_positions pos JOIN pumphound.simulation_runs r USING(simulation_run_id)
      JOIN pumphound.portfolios p ON p.portfolio_id=pos.portfolio_id
      LEFT JOIN pumphound.assets a ON a.mint=pos.base_mint
      LEFT JOIN pumphound.assets qa ON qa.mint=pos.quote_mint
      LEFT JOIN LATERAL (
        SELECT quarantine_id,reason_code,evidence
        FROM pumphound.entity_integrity_quarantines iq
        WHERE iq.status='ACTIVE' AND (
          (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
          OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
        )
        ORDER BY CASE WHEN iq.entity_type='POSITION' THEN 0 ELSE 1 END,iq.detected_at
        LIMIT 1
      ) iq ON true
      LEFT JOIN LATERAL (
        SELECT d.decision_id,o.order_id,f.fill_id,d.feature_snapshot_id,d.model_versions,d.policy_version,
          jsonb_build_object(
            'decision_id',d.decision_id,'decision_at',d.decision_at,
            'knowledge_cutoff',d.knowledge_cutoff,'policy_version',d.policy_version,
            'feature_snapshot_id',d.feature_snapshot_id,'model_versions',d.model_versions,
            'order_id',o.order_id,'fill_id',f.fill_id,'filled_at',f.arrival_at,
            'input_atomic',f.input_atomic::text,'input_mint',f.input_mint,
            'output_atomic',f.output_atomic::text,'output_mint',f.output_mint
          ) v
        FROM pumphound.acquisition_lots lot
        JOIN pumphound.paper_fills f ON f.fill_id=lot.fill_id
        JOIN pumphound.paper_orders o ON o.order_id=f.order_id
        JOIN pumphound.decisions d ON d.decision_id=o.decision_id
        WHERE lot.simulation_run_id=pos.simulation_run_id
          AND lot.base_mint=pos.base_mint AND o.side='buy'
        ORDER BY lot.acquired_at,o.accepted_at LIMIT 1
      ) entry ON true
      LEFT JOIN LATERAL (SELECT jsonb_build_object('at',d.decision_at,'state','HISTORICAL',
        'mark_return_fraction',d.exact_inputs->'mark_return_fraction','decision_id',d.decision_id,
        'reason','Last persisted exit evaluation; not a current executable quote') v
        FROM pumphound.decisions d WHERE d.payload->>'position_id'=pos.position_id::text
        AND d.decision_kind='EXIT' ORDER BY d.decision_at DESC LIMIT 1) mark ON true
      WHERE pos.simulation_run_id=ANY($1) AND ($2::text IS NULL OR pos.status=$2 OR ($2='OPEN' AND pos.status='UNMARKABLE'))
       AND ($3::timestamptz IS NULL OR COALESCE(pos.closed_at,pos.opened_at)>=$3)) rows"#)
      .bind(&ids).bind(state.as_deref()).bind(since).fetch_one(&mut *tx).await?};
    let summary:Value=sqlx::query_scalar(r#"WITH selected AS (
       SELECT pos.* FROM pumphound.paper_positions pos
       WHERE simulation_run_id=ANY($1) AND ($2::text IS NULL OR status=$2 OR ($2='OPEN' AND status='UNMARKABLE'))
       AND ($3::timestamptz IS NULL OR COALESCE(closed_at,opened_at)>=$3)
       AND NOT EXISTS (
         SELECT 1 FROM pumphound.entity_integrity_quarantines iq
         WHERE iq.status='ACTIVE' AND (
           (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
           OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
         )
       )), money AS (
       SELECT quote_mint, SUM(realized_pnl_quote_atomic)::text atomic FROM selected GROUP BY quote_mint
      ), closed_lots AS (
       SELECT lot.lot_id,MAX(sell_fill.arrival_at) closed_at
       FROM pumphound.acquisition_lots lot
       JOIN pumphound.paper_fills entry_fill ON entry_fill.fill_id=lot.fill_id
       JOIN pumphound.paper_orders entry_order ON entry_order.order_id=entry_fill.order_id
       JOIN pumphound.decisions entry_decision ON entry_decision.decision_id=entry_order.decision_id
       JOIN selected pos
         ON pos.simulation_run_id=lot.simulation_run_id AND pos.base_mint=lot.base_mint
       JOIN pumphound.lot_consumptions c ON c.lot_id=lot.lot_id
       JOIN pumphound.paper_fills sell_fill ON sell_fill.fill_id=c.sell_fill_id
       WHERE NOT EXISTS (
         SELECT 1 FROM pumphound.entity_integrity_quarantines iq
         WHERE iq.status='ACTIVE' AND (
           (iq.entity_type='LOT' AND iq.entity_id=lot.lot_id::text)
           OR (iq.entity_type='LAUNCH' AND iq.entity_id=entry_decision.launch_id::text)
         )
       )
       GROUP BY lot.lot_id,lot.base_atomic
       HAVING SUM(c.base_atomic)=lot.base_atomic
      )
      SELECT jsonb_build_object('open_positions',(SELECT count(*) FROM selected WHERE status IN ('OPEN','UNMARKABLE')),
       'closed_positions',(SELECT count(*) FROM selected WHERE status='CLOSED'),
       'closed_positions_today',(SELECT count(*) FROM selected WHERE status='CLOSED' AND closed_at>=date_trunc('day',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'),
       'closed_trades',(SELECT count(*) FROM closed_lots),
       'closed_trades_today',(SELECT count(*) FROM closed_lots WHERE closed_at>=date_trunc('day',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'),
       'closed_trade_basis','FULLY_FIFO_CONSUMED_ENTRY_LOT',
       'integrity_quarantined_positions',(SELECT count(*) FROM pumphound.paper_positions p
          WHERE p.simulation_run_id=ANY($1) AND EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
            ))),
       'integrity_quarantined_orders',(SELECT count(*) FROM pumphound.paper_orders o
          JOIN pumphound.decisions od ON od.decision_id=o.decision_id
          WHERE o.simulation_run_id=ANY($1) AND EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=od.launch_id::text)
            ))),
       'realized_pnl_quote_atomic',COALESCE((SELECT atomic FROM money WHERE quote_mint=$4),'0'),
       'realized_pnl_by_asset',COALESCE((SELECT jsonb_agg(jsonb_build_object('asset_mint',quote_mint,'atomic',atomic,'decimals',a.decimals,'symbol',a.symbol)) FROM money LEFT JOIN pumphound.assets a ON a.mint=quote_mint),'[]'),
       'capital','SIMULATED','unrealized_pnl',null,'unrealized_pnl_state','UNKNOWN',
       'unrealized_pnl_reason','Current executable marks are not persisted for every holding')"#)
      .bind(&ids).bind(state.as_deref()).bind(since).bind(pumphound_domain::WSOL_MINT).fetch_one(&mut *tx).await?;
    let orders:Value=if q.summary_only { empty.clone() } else { sqlx::query_scalar(r#"SELECT COALESCE(jsonb_agg(v ORDER BY v->>'accepted_at' DESC),'[]') FROM (
      SELECT (to_jsonb(o)-'payload')||jsonb_build_object('input_atomic',o.input_atomic::text,
       'minimum_output_atomic',o.minimum_output_atomic::text,'input_decimals',a.decimals,
       'book_id',o.simulation_run_id,'experiment_id',r.manifest->>'experiment_id','strategy_version',r.manifest->>'strategy',
       'policy_version',d.policy_version,'launch_id',d.launch_id,'trade_trace_id',COALESCE(pos.position_id,o.decision_id),
       'integrity_state',CASE WHEN iq.quarantine_id IS NULL THEN 'VERIFIED' ELSE 'QUARANTINED' END,
       'integrity_reason',iq.reason_code,'integrity_evidence',iq.evidence) v
      FROM pumphound.paper_orders o JOIN pumphound.decisions d USING(decision_id)
      JOIN pumphound.simulation_runs r ON r.simulation_run_id=o.simulation_run_id
      JOIN pumphound.assets a ON a.mint=o.input_mint
      LEFT JOIN pumphound.paper_positions pos ON pos.simulation_run_id=o.simulation_run_id AND pos.launch_id=d.launch_id AND pos.market=o.market
      LEFT JOIN LATERAL (
        SELECT quarantine_id,reason_code,evidence
        FROM pumphound.entity_integrity_quarantines iq
        WHERE iq.status='ACTIVE' AND (
          (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
          OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
        )
        ORDER BY CASE WHEN iq.entity_type='ORDER' THEN 0 ELSE 1 END,iq.detected_at
        LIMIT 1
      ) iq ON true
      WHERE o.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR o.accepted_at>=$2)
       AND ($3::text IS NULL OR pos.status=$3 OR ($3='OPEN' AND pos.status='UNMARKABLE'))
      ORDER BY o.accepted_at DESC LIMIT 1000) rows"#).bind(&ids).bind(since).bind(state.as_deref()).fetch_one(&mut *tx).await?};
    let activity:Value=if q.summary_only { empty } else { sqlx::query_scalar(r#"WITH decision_events AS (
      SELECT d.decision_id::text event_id,
       CASE WHEN d.decision_kind='EXIT' THEN 'EXIT_DECISION' ELSE 'STRATEGY_DECISION' END kind,
       d.decision_at at,d.simulation_run_id book_id,d.launch_id,
       COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('decision_id',d.decision_id,'action',d.action,'reasons',d.reason_codes,
         'policy_version',d.policy_version,'feature_snapshot_id',d.feature_snapshot_id,
         'model_versions',d.model_versions) payload
      FROM pumphound.decisions d
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=d.simulation_run_id AND pos.launch_id=d.launch_id
      WHERE d.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR d.decision_at>=$2)
    ), order_created AS (
      SELECT o.order_id::text||':created' event_id,'ORDER_CREATED' kind,o.accepted_at at,
       o.simulation_run_id book_id,d.launch_id,COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('order_id',o.order_id,'decision_id',d.decision_id,'side',o.side,
         'status',o.status,'input_mint',o.input_mint,'input_atomic',o.input_atomic::text,
         'output_mint',o.output_mint) payload
      FROM pumphound.paper_orders o JOIN pumphound.decisions d USING(decision_id)
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=o.simulation_run_id AND pos.launch_id=d.launch_id AND pos.market=o.market
      WHERE o.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR o.accepted_at>=$2)
    ), order_events AS (
      SELECT e.order_event_id::text event_id,
       CASE WHEN e.status LIKE 'ORDER_%' THEN e.status ELSE 'ORDER_'||e.status END kind,
       e.at_time at,o.simulation_run_id book_id,d.launch_id,
       COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('order_id',o.order_id,'decision_id',d.decision_id,
         'status',e.status,'reason',e.reason_code,'details',e.payload) payload
      FROM pumphound.paper_order_events e JOIN pumphound.paper_orders o USING(order_id)
      JOIN pumphound.decisions d USING(decision_id)
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=o.simulation_run_id AND pos.launch_id=d.launch_id AND pos.market=o.market
      WHERE o.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR e.at_time>=$2)
    ), fill_events AS (
      SELECT f.fill_id::text event_id,CASE WHEN o.side='sell' THEN 'EXIT_FILL' ELSE 'FILL' END kind,
       f.arrival_at at,o.simulation_run_id book_id,d.launch_id,
       COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('fill_id',f.fill_id,'order_id',o.order_id,'decision_id',d.decision_id,
         'side',o.side,'input_mint',f.input_mint,'input_atomic',f.input_atomic::text,
         'output_mint',f.output_mint,'output_atomic',f.output_atomic::text,
         'market_snapshot_id',f.market_snapshot_id,'fees',f.fee_components) payload
      FROM pumphound.paper_fills f JOIN pumphound.paper_orders o USING(order_id)
      JOIN pumphound.decisions d USING(decision_id)
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=o.simulation_run_id AND pos.launch_id=d.launch_id AND pos.market=o.market
      WHERE o.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR f.arrival_at>=$2)
    ), pnl_events AS (
      SELECT c.consumption_id::text event_id,'REALIZED_PNL' kind,sell_fill.arrival_at at,
       lot.simulation_run_id book_id,d.launch_id,pos.position_id trade_trace_id,
       jsonb_build_object('consumption_id',c.consumption_id,'lot_id',c.lot_id,'sell_fill_id',c.sell_fill_id,
         'base_atomic',c.base_atomic::text,'allocated_cost_atomic',c.allocated_cost_atomic::text,
         'net_proceeds_atomic',c.net_proceeds_atomic::text,
         'realized_pnl_atomic',c.realized_pnl_atomic::text,'quote_mint',lot.quote_mint) payload
      FROM pumphound.lot_consumptions c
      JOIN pumphound.acquisition_lots lot USING(lot_id)
      JOIN pumphound.paper_fills sell_fill ON sell_fill.fill_id=c.sell_fill_id
      JOIN pumphound.paper_orders o ON o.order_id=sell_fill.order_id
      JOIN pumphound.decisions d ON d.decision_id=o.decision_id
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=lot.simulation_run_id AND pos.base_mint=lot.base_mint
      WHERE lot.simulation_run_id=ANY($1)
        AND ($2::timestamptz IS NULL OR sell_fill.arrival_at>=$2)
    ), position_events AS (
      SELECT pos.position_id::text||':open' event_id,'POSITION_OPENED' kind,pos.opened_at at,
       pos.simulation_run_id book_id,pos.launch_id,pos.position_id trade_trace_id,
       jsonb_build_object('position_id',pos.position_id,'market',pos.market,'base_mint',pos.base_mint,
         'quote_mint',pos.quote_mint,'cost_quote_atomic',pos.cost_quote_atomic::text) payload
      FROM pumphound.paper_positions pos
      WHERE pos.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR pos.opened_at>=$2)
      UNION ALL
      SELECT pos.position_id::text||':closed','POSITION_CLOSED',pos.closed_at,
       pos.simulation_run_id,pos.launch_id,pos.position_id,
       jsonb_build_object('position_id',pos.position_id,'realized_pnl_quote_atomic',
         pos.realized_pnl_quote_atomic::text,'quote_mint',pos.quote_mint,
         'last_exit_decision_id',pos.last_exit_decision_id)
      FROM pumphound.paper_positions pos
      WHERE pos.simulation_run_id=ANY($1) AND pos.closed_at IS NOT NULL
        AND ($2::timestamptz IS NULL OR pos.closed_at>=$2)
    ), model_events AS (
      SELECT DISTINCT ON (p.prediction_id,d.simulation_run_id)
       p.prediction_id||':'||d.simulation_run_id::text event_id,'MODEL_PREDICTION' kind,p.computed_at at,
       d.simulation_run_id book_id,d.launch_id,COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('prediction_id',p.prediction_id,'model_version',p.model_version,
         'feature_snapshot_id',p.feature_snapshot_id,'familiarity',p.familiarity,
         'abstention_reason',p.abstention_reason,'outputs',p.outputs) payload
      FROM pumphound.model_predictions p JOIN pumphound.decisions d USING(feature_snapshot_id)
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=d.simulation_run_id AND pos.launch_id=d.launch_id
      WHERE d.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR p.computed_at>=$2)
      ORDER BY p.prediction_id,d.simulation_run_id,d.decision_at
    ), label_events AS (
      SELECT DISTINCT ON (label.outcome_label_id,d.simulation_run_id)
       label.outcome_label_id||':'||d.simulation_run_id::text event_id,'LABEL_MATURED' kind,
       label.matured_at at,d.simulation_run_id book_id,d.launch_id,
       COALESCE(pos.position_id,d.decision_id) trade_trace_id,
       jsonb_build_object('outcome_label_id',label.outcome_label_id,'target_version',
         label.target_version,'feature_snapshot_id',label.feature_snapshot_id,
         'maturity_due_at',label.maturity_due_at,'value',label.value,
         'evidence',label.payload) payload
      FROM pumphound.outcome_labels label
      JOIN pumphound.decisions d USING(feature_snapshot_id)
      LEFT JOIN pumphound.paper_positions pos
        ON pos.simulation_run_id=d.simulation_run_id AND pos.launch_id=d.launch_id
      WHERE label.status='MATURED' AND d.simulation_run_id=ANY($1)
        AND ($2::timestamptz IS NULL OR label.matured_at>=$2)
      ORDER BY label.outcome_label_id,d.simulation_run_id,d.decision_at
    ), risk_events AS (
      SELECT risk.risk_snapshot_id::text event_id,'RISK_UPDATE' kind,risk.at_time at,
       risk.simulation_run_id book_id,NULL::uuid launch_id,NULL::uuid trade_trace_id,
       jsonb_build_object('risk_snapshot_id',risk.risk_snapshot_id,'policy_version',
         risk.policy_version,'gates',risk.gates,'exposures',risk.exposures,
         'breaker_state',risk.breaker_state) payload
      FROM pumphound.risk_snapshots risk
      WHERE risk.simulation_run_id=ANY($1) AND ($2::timestamptz IS NULL OR risk.at_time>=$2)
    ), all_events AS (
      SELECT * FROM decision_events UNION ALL SELECT * FROM order_created
      UNION ALL SELECT * FROM order_events UNION ALL SELECT * FROM fill_events
      UNION ALL SELECT * FROM pnl_events
      UNION ALL SELECT * FROM position_events UNION ALL SELECT * FROM model_events
      UNION ALL SELECT * FROM label_events UNION ALL SELECT * FROM risk_events
    ), verified_events AS (
      SELECT all_events.*
      FROM all_events
      LEFT JOIN LATERAL (
        SELECT 1 quarantined
        FROM pumphound.entity_integrity_quarantines iq
        WHERE iq.status='ACTIVE' AND (
          (iq.entity_type='LAUNCH' AND iq.entity_id=all_events.launch_id::text)
          OR (iq.entity_type='POSITION' AND iq.entity_id=all_events.trade_trace_id::text)
          OR (iq.entity_type='ORDER' AND iq.entity_id=all_events.payload->>'order_id')
          OR (iq.entity_type='LOT' AND iq.entity_id=all_events.payload->>'lot_id')
        )
        LIMIT 1
      ) integrity ON true
      WHERE integrity.quarantined IS NULL
    ), ranked AS (
      SELECT verified_events.*,
        row_number() OVER (
          PARTITION BY CASE
            WHEN kind LIKE '%DECISION' THEN 'DECISIONS'
            WHEN kind LIKE 'ORDER_%' THEN 'ORDERS'
            WHEN kind LIKE '%FILL' OR kind='REALIZED_PNL' THEN 'FILLS'
            WHEN kind LIKE 'POSITION_%' THEN 'POSITIONS'
            WHEN kind LIKE 'MODEL_%' OR kind LIKE 'LABEL_%' THEN 'LEARNING'
            WHEN kind LIKE 'RISK_%' THEN 'RISK'
            ELSE 'OTHER'
          END
          ORDER BY at DESC,event_id
        ) source_rank
      FROM verified_events
    )
    SELECT COALESCE(jsonb_agg(to_jsonb(x)||jsonb_build_object(
      'experiment_id',r.manifest->>'experiment_id','strategy_version',r.manifest->>'strategy')
      ORDER BY x.at DESC,x.event_id),'[]')
    FROM (SELECT * FROM ranked WHERE source_rank<=80 ORDER BY at DESC,event_id) x
    JOIN pumphound.simulation_runs r ON r.simulation_run_id=x.book_id"#)
      .bind(&ids).bind(since).fetch_one(&mut *tx).await?};
    tx.commit().await?;
    let mut summary = summary;
    let atomic = summary["realized_pnl_quote_atomic"].as_str().unwrap_or("0");
    summary["realized_pnl"] = atomic
        .parse::<i128>()
        .map(crate::ledger_metrics::sol_money)
        .unwrap_or_else(|_| {
            json!({
                "atomic": atomic,
                "asset_mint": pumphound_domain::WSOL_MINT,
                "decimals": 9,
                "symbol": "SOL",
                "status": "ERROR",
                "display": "UNKNOWN"
            })
        });
    Ok(
        json!({"contract_version":"product-truth-v1","scope":{"label":if q.book_id.is_none() && q.experiment_id.is_none() && q.strategy.is_none() {"ALL ACTIVE PAPER BOOKS"} else {"FILTERED ACTIVE PAPER BOOKS"},"book_ids":ids,"experiment_id":q.experiment_id,"strategy":q.strategy,"state":state.as_deref().unwrap_or("ALL"),"window":q.window.as_deref().unwrap_or("all"),"timezone":"UTC","as_of":now,"active_book_definition":"running simulation run; unarchived portfolio; strategy in manifest","position_window":"closed_at for closed; opened_at for open","activity_scope":"selected books and time; independent of position state"},"limits":{"orders":1000,"activity":"latest 80 per durable activity category"},"summary":summary,"books":books,"available_books":all_books,"positions":positions,"orders":orders,"activity":activity,"paper_only":true}),
    )
}

pub async fn trace(stores: &Stores, id: Uuid, q: &TraceQuery) -> anyhow::Result<Value> {
    let mut tx = stores.postgres.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let identity: Option<Value> = sqlx::query_scalar(
        r#"WITH requested AS (
      SELECT pos.position_id,NULL::uuid requested_decision_id,pos.simulation_run_id,pos.launch_id
      FROM pumphound.paper_positions pos WHERE pos.position_id=$1
      UNION ALL
      SELECT NULL::uuid,d.decision_id,d.simulation_run_id,d.launch_id
      FROM pumphound.decisions d WHERE d.decision_id=$1
        AND NOT EXISTS (SELECT 1 FROM pumphound.paper_positions pos WHERE pos.position_id=$1)
      LIMIT 1
    ), resolved AS (
      SELECT requested.*,COALESCE(requested.position_id,matched.position_id) resolved_position_id
      FROM requested LEFT JOIN LATERAL (
        SELECT linked.position_id FROM (
          SELECT pos.position_id,pos.opened_at
          FROM pumphound.paper_orders o
          JOIN pumphound.paper_fills f ON f.order_id=o.order_id
          JOIN pumphound.acquisition_lots lot ON lot.fill_id=f.fill_id
          JOIN pumphound.paper_positions pos
            ON pos.simulation_run_id=lot.simulation_run_id AND pos.base_mint=lot.base_mint
          WHERE o.decision_id=requested.requested_decision_id AND o.side='buy'
          UNION ALL
          SELECT pos.position_id,pos.opened_at
          FROM pumphound.decisions d
          JOIN pumphound.paper_positions pos
            ON pos.position_id::text=d.payload->>'position_id'
          WHERE d.decision_id=requested.requested_decision_id AND d.decision_kind='EXIT'
        ) linked ORDER BY linked.opened_at LIMIT 1
      ) matched ON true
    )
    SELECT jsonb_build_object(
       'trade_trace_id',COALESCE(x.resolved_position_id,x.requested_decision_id),
       'position_id',pos.position_id,'requested_decision_id',x.requested_decision_id,
       'book_id',r.simulation_run_id,'portfolio_id',r.portfolio_id,
       'experiment_id',r.manifest->>'experiment_id','strategy_version',r.manifest->>'strategy',
       'launch_id',l.launch_id,'mint',l.mint,'market',pos.market,
       'policy_version',COALESCE(requested_d.policy_version,entry_d.policy_version),
       'entry_decision_id',entry_d.decision_id,
       'decision_at',COALESCE(requested_d.decision_at,entry_d.decision_at),
       'knowledge_cutoff',COALESCE(requested_d.knowledge_cutoff,entry_d.knowledge_cutoff),
       'feature_snapshot_id',COALESCE(requested_d.feature_snapshot_id,entry_d.feature_snapshot_id),
       'model_versions',COALESCE(requested_d.model_versions,entry_d.model_versions,'{}'::jsonb),
       'integrity_state',CASE WHEN iq.quarantine_id IS NULL THEN 'VERIFIED' ELSE 'QUARANTINED' END,
       'integrity_reason',iq.reason_code,'integrity_evidence',iq.evidence)
      FROM resolved x
      JOIN pumphound.simulation_runs r ON r.simulation_run_id=x.simulation_run_id
      JOIN pumphound.launches l ON l.launch_id=x.launch_id
      LEFT JOIN pumphound.paper_positions pos ON pos.position_id=x.resolved_position_id
      LEFT JOIN LATERAL (
        SELECT quarantine_id,reason_code,evidence
        FROM pumphound.entity_integrity_quarantines iq
        WHERE iq.status='ACTIVE' AND (
          (iq.entity_type='POSITION' AND iq.entity_id=x.resolved_position_id::text)
          OR (iq.entity_type='LAUNCH' AND iq.entity_id=x.launch_id::text)
        )
        ORDER BY CASE WHEN iq.entity_type='POSITION' THEN 0 ELSE 1 END,iq.detected_at
        LIMIT 1
      ) iq ON true
      LEFT JOIN pumphound.decisions requested_d ON requested_d.decision_id=x.requested_decision_id
      LEFT JOIN LATERAL (
        SELECT d.* FROM pumphound.acquisition_lots lot
        JOIN pumphound.paper_fills f ON f.fill_id=lot.fill_id
        JOIN pumphound.paper_orders o ON o.order_id=f.order_id
        JOIN pumphound.decisions d ON d.decision_id=o.decision_id
        WHERE lot.simulation_run_id=x.simulation_run_id AND d.launch_id=x.launch_id
          AND o.side='buy' AND (pos.base_mint IS NULL OR lot.base_mint=pos.base_mint)
        ORDER BY lot.acquired_at,o.accepted_at LIMIT 1
      ) entry_d ON true
      LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(identity) = identity else {
        return Ok(
            json!({"identity":{"trade_trace_id":id},"events":[],"completeness":{"state":"REPLAY_INCOMPLETE","missing":[format!("No persisted position or strategy decision for {id}")]},"virtual_time":q.at}),
        );
    };
    let run = Uuid::parse_str(identity["book_id"].as_str().unwrap())?;
    let launch = Uuid::parse_str(identity["launch_id"].as_str().unwrap())?;
    let events:Value=sqlx::query_scalar(r#"WITH ds AS (SELECT * FROM pumphound.decisions WHERE simulation_run_id=$1 AND launch_id=$2),
      os AS (SELECT o.* FROM pumphound.paper_orders o JOIN ds d USING(decision_id)),
      fs AS (SELECT f.* FROM pumphound.feature_snapshots f WHERE f.feature_snapshot_id IN (SELECT feature_snapshot_id FROM ds)),
      events AS (
      SELECT l.creation_event_id event_id,'LAUNCH' kind,COALESCE(l.creation_time,l.known_at) at,l.known_at known_at,
        to_jsonb(l) payload FROM pumphound.launches l WHERE l.launch_id=$2
      UNION ALL SELECT f.feature_snapshot_id,'FEATURE_SNAPSHOT',f.knowledge_cutoff,GREATEST(f.knowledge_cutoff,f.computed_at),to_jsonb(f) FROM fs f
      UNION ALL SELECT p.prediction_id,'MODEL_PREDICTION',p.computed_at,p.computed_at,to_jsonb(p)||jsonb_build_object('target_version',m.target_version,
        'model_artifact',m.manifest->'artifacts','registry_role_now',m.status,'registry_role_is_historical',false)
        FROM pumphound.model_predictions p JOIN pumphound.model_registry m USING(model_version) WHERE p.feature_snapshot_id IN (SELECT feature_snapshot_id FROM fs)
      UNION ALL SELECT d.decision_id::text,CASE WHEN d.decision_kind='EXIT' THEN 'EXIT_DECISION' ELSE 'STRATEGY_DECISION' END,
        d.decision_at,GREATEST(d.decision_at,d.knowledge_cutoff),to_jsonb(d) FROM ds d
      UNION ALL SELECT o.order_id::text,'ORDER_CREATED',o.accepted_at,o.accepted_at,
        (to_jsonb(o)-'arrival_quote'-'arrival_snapshot_id'-'arrived_at'-'settled_at'-'rejection_reason'-'status')||jsonb_build_object('input_atomic',o.input_atomic::text,'minimum_output_atomic',o.minimum_output_atomic::text) FROM os o
      UNION ALL SELECT e.order_event_id::text,e.status,e.at_time,e.at_time,to_jsonb(e) FROM pumphound.paper_order_events e JOIN os USING(order_id)
      UNION ALL SELECT o.order_id::text||':arrival','ARRIVAL_QUOTE',o.arrived_at,o.arrived_at,
        jsonb_build_object('order_id',o.order_id,'arrival_quote',o.arrival_quote,'market_snapshot_id',o.arrival_snapshot_id) FROM os o WHERE o.arrived_at IS NOT NULL AND o.arrival_quote IS NOT NULL
      UNION ALL SELECT f.fill_id::text,CASE WHEN o.side='sell' THEN 'EXIT_FILL' ELSE 'FILL' END,f.arrival_at,f.arrival_at,
        to_jsonb(f)||jsonb_build_object('input_atomic',f.input_atomic::text,'output_atomic',f.output_atomic::text) FROM pumphound.paper_fills f JOIN os o USING(order_id)
      UNION ALL SELECT pos.position_id::text||':open','POSITION_OPENED',pos.opened_at,pos.opened_at,
        jsonb_build_object('position_id',pos.position_id,'launch_id',pos.launch_id,'market',pos.market,'base_mint',pos.base_mint,'quote_mint',pos.quote_mint)
        FROM pumphound.paper_positions pos WHERE pos.simulation_run_id=$1 AND pos.launch_id=$2
      UNION ALL SELECT pos.position_id::text||':closed','POSITION_CLOSED',pos.closed_at,pos.closed_at,
        jsonb_build_object('position_id',pos.position_id,'realized_pnl_quote_atomic',pos.realized_pnl_quote_atomic::text,'quote_mint',pos.quote_mint,'last_exit_decision_id',pos.last_exit_decision_id)
        FROM pumphound.paper_positions pos WHERE pos.simulation_run_id=$1 AND pos.launch_id=$2 AND pos.closed_at IS NOT NULL
      UNION ALL SELECT c.consumption_id::text,'REALIZED_PNL',f.arrival_at,f.arrival_at,
        to_jsonb(c)||jsonb_build_object('base_atomic',c.base_atomic::text,'allocated_cost_atomic',c.allocated_cost_atomic::text,'net_proceeds_atomic',c.net_proceeds_atomic::text,'realized_pnl_atomic',c.realized_pnl_atomic::text,'quote_mint',lot.quote_mint)
        FROM pumphound.lot_consumptions c JOIN pumphound.paper_fills f ON f.fill_id=c.sell_fill_id JOIN os ON os.order_id=f.order_id JOIN pumphound.acquisition_lots lot USING(lot_id)
      UNION ALL SELECT label.outcome_label_id,'LABEL_MATURED',label.maturity_due_at,label.matured_at,to_jsonb(label)
        FROM pumphound.outcome_labels label WHERE label.feature_snapshot_id IN (SELECT feature_snapshot_id FROM fs) AND label.status='MATURED'
      UNION ALL SELECT e.lifecycle_event_id::text,e.state,e.occurred_at,e.occurred_at,to_jsonb(e)
        FROM pumphound.candidate_lifecycle_events e WHERE e.decision_id IN (SELECT decision_id FROM ds)
      UNION ALL SELECT risk.risk_snapshot_id::text,'RISK_UPDATE',risk.at_time,risk.at_time,to_jsonb(risk)
        FROM pumphound.risk_snapshots risk WHERE risk.risk_snapshot_id IN (SELECT risk_snapshot_id FROM ds)
      ) SELECT COALESCE(jsonb_agg(to_jsonb(events) ORDER BY known_at,event_id),'[]') FROM events"#)
      .bind(run).bind(launch).fetch_one(&mut *tx).await?;
    let missing_features:Vec<String>=sqlx::query_scalar(r#"SELECT DISTINCT d.feature_snapshot_id FROM pumphound.decisions d LEFT JOIN pumphound.feature_snapshots f USING(feature_snapshot_id)
      WHERE d.simulation_run_id=$1 AND d.launch_id=$2 AND f.feature_snapshot_id IS NULL AND ($3::timestamptz IS NULL OR d.decision_at<=$3)"#)
      .bind(run).bind(launch).bind(q.at).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut events = events.as_array().cloned().unwrap_or_default();
    let cutoff =
        event_time(&identity, "knowledge_cutoff").or_else(|| event_time(&identity, "decision_at"));
    apply_time(&mut events, q.at, cutoff);
    let mut missing: Vec<String> = missing_features
        .into_iter()
        .map(|id| format!("Historical feature snapshot unavailable: {id}"))
        .collect();
    if identity["integrity_state"] == "QUARANTINED" {
        missing.push(format!(
            "Entity integrity quarantine is active: {}",
            identity["integrity_reason"]
                .as_str()
                .unwrap_or("UNKNOWN_REASON")
        ));
    }
    if !events.iter().any(|e| e["kind"] == "FEATURE_SNAPSHOT") {
        missing.push("No persisted feature snapshot available at this virtual time".into());
    }
    if !events.iter().any(|e| e["kind"] == "STRATEGY_DECISION") {
        missing.push("No strategy decision available at this virtual time".into());
    }
    // Tape is separately retained in ClickHouse; exact snapshot IDs from orders and features are the join keys.
    let snapshot_ids: Vec<String> = events
        .iter()
        .flat_map(|event| {
            [
                "market_snapshot_id",
                "decision_snapshot_id",
                "arrival_snapshot_id",
            ]
            .into_iter()
            .filter_map(|key| event["payload"][key].as_str().map(str::to_owned))
        })
        .collect();
    let mut market_evidence = Vec::new();
    for snapshot_id in snapshot_ids
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        #[derive(Deserialize, clickhouse::Row)]
        struct Snapshot {
            snapshot_json: String,
        }
        match stores.clickhouse.query("SELECT snapshot_json FROM market_snapshots WHERE snapshot_id=? ORDER BY computed_at DESC LIMIT 1").bind(&snapshot_id).fetch_optional::<Snapshot>().await {
            Ok(Some(row))=>match serde_json::from_str::<Value>(&row.snapshot_json) {
                Ok(payload)=> {let known=payload.get("computed_at").or_else(||payload.get("knowledge_cutoff")).cloned().unwrap_or(Value::Null);let at=payload.get("knowledge_cutoff").or_else(||payload.get("observed_at")).cloned().unwrap_or(Value::Null);
                    market_evidence.push(json!({"event_id":snapshot_id,"kind":"MARKET_SNAPSHOT","at":at,"known_at":known,"payload":payload}));},
                Err(_)=>missing.push(format!("Market snapshot {snapshot_id} payload cannot be decoded"))},
            Ok(None)=>missing.push(format!("Historical market snapshot unavailable: {snapshot_id}")),
            Err(_)=>{missing.push("Historical market snapshot store unavailable".into());break;}
        }
    }
    apply_time(&mut market_evidence, q.at, cutoff);
    events.extend(market_evidence);
    apply_time(&mut events, q.at, cutoff);
    if !events.iter().any(|e| e["kind"] == "MARKET_SNAPSHOT") {
        missing.push(
            "Executable historical market snapshots unavailable; price/tape overlay incomplete"
                .into(),
        );
    }
    missing.push("Full market tape continuity and intervening slot coverage have not been certified; this is an evidence replay, not a re-simulation".into());
    if !events.iter().any(|e| e["kind"] == "LABEL_MATURED") {
        missing.push(
            "No matured label linked to these decision snapshots at this virtual time".into(),
        );
    }
    // Current registry role is not evidence that a model had that role at the decision time.
    let mut identity = identity;
    if q.at
        .is_some_and(|at| event_time(&identity, "decision_at").is_some_and(|t| t > at))
    {
        for key in [
            "policy_version",
            "entry_decision_id",
            "model_versions",
            "decision_at",
            "knowledge_cutoff",
        ] {
            identity[key] = Value::Null;
        }
    }
    Ok(
        json!({"identity":identity,"virtual_time":q.at,"events":events,"completeness":{"state":if missing.is_empty(){"EVIDENCE_AVAILABLE"}else{"REPLAY_INCOMPLETE"},"missing":missing},"temporal_contract":"Only events both occurred and known by virtual_time are returned. Embedded decision inputs preserve what the strategy actually consumed. Registry role now is not historical authority.","paper_only":true}),
    )
}
