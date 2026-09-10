use crate::store;
use pumphound_storage::Stores;
use pumphound_venue_core::{HealthState, Venue};
use pumphound_venue_polymarket::{relations_from_markets, PolymarketClient, RelationProvenance};

pub async fn tick(stores: &Stores) -> anyhow::Result<()> {
    let client = PolymarketClient::default();
    let markets = client.active_markets(40).await?;
    for market in &markets {
        if let Some(event_id) = &market.event_id {
            store::upsert_prediction_event(
                stores,
                event_id,
                market.event_title.as_deref(),
                None,
                market.category.as_deref(),
                serde_json::json!({"from":"gamma.events"}),
            )
            .await?;
        }
        store::upsert_instrument(
            stores,
            Venue::Polymarket,
            &market.market_id,
            &market.question,
            "outcome",
            None,
            None,
            None,
            market.closed.unwrap_or(false),
            market.end_date,
            serde_json::json!({"event_id": market.event_id}),
        )
        .await?;
        store::upsert_prediction_market(
            stores,
            &market.market_id,
            market.event_id.as_deref(),
            &market.question,
            market.yes_token_id.as_deref(),
            market.no_token_id.as_deref(),
            market.yes_price,
            market.no_price,
            market.best_bid,
            market.best_ask,
            market.spread,
            market.volume,
            market.liquidity,
            market.end_date,
            market.resolution_source.as_deref(),
            market.resolution_rules.as_deref(),
            market.resolved_by.as_deref(),
            market.active,
            market.closed,
            serde_json::json!({
                "condition_id": market.condition_id,
                "resolution_source_empty_means_unknown": true
            }),
        )
        .await?;
    }

    for rel in relations_from_markets(&markets) {
        let provenance = match rel.provenance {
            RelationProvenance::ProvenRelation => "PROVEN_RELATION",
            RelationProvenance::LikelyRelation => "LIKELY_RELATION",
            RelationProvenance::UnverifiedRelation => "UNVERIFIED_RELATION",
        };
        store::upsert_relation(
            stores,
            &rel.left_id,
            &rel.right_id,
            &rel.relation_type,
            provenance,
            rel.evidence,
        )
        .await?;
    }

    let mut books = 0u32;
    for market in markets.iter().take(8) {
        if let Some(token) = &market.yes_token_id {
            match client.book(token).await {
                Ok(book) => {
                    store::insert_prediction_book(
                        stores,
                        token,
                        book.best_bid,
                        book.best_ask,
                        book.mid,
                        book.spread,
                        serde_json::json!({
                            "bids": book.bids.iter().take(10).map(|(p,s)| serde_json::json!([p.as_f64(), s.as_f64()])).collect::<Vec<_>>(),
                            "asks": book.asks.iter().take(10).map(|(p,s)| serde_json::json!([p.as_f64(), s.as_f64()])).collect::<Vec<_>>(),
                            "market": book.market
                        }),
                    )
                    .await?;
                    books += 1;
                }
                Err(error) => tracing::warn!(token, %error, "clob book failed"),
            }
        }
    }

    store::upsert_health(
        stores,
        Venue::Polymarket,
        "gamma",
        HealthState::Healthy,
        None,
        markets.len() as i64,
        markets.len() as i32,
        serde_json::json!({"books_sampled": books, "proven_relations_from_titles": false}),
    )
    .await?;
    Ok(())
}
