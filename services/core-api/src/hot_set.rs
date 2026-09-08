use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

pub const HOT_SET_CAPACITY: i64 = 64;
pub const HOT_SET_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Debug)]
pub struct HotMembership {
    pub mint: String,
    pub launch_id: Option<Uuid>,
    pub reason_code: String,
    pub promoted_by: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

pub async fn promote(
    stores: &Stores,
    mint: &str,
    launch_id: Option<Uuid>,
    reason_code: &str,
    promoted_by: &str,
) -> anyhow::Result<Value> {
    let canonical_mint = if let Some(launch_id) = launch_id {
        let launch_mint = sqlx::query_scalar::<_, String>(
            r#"SELECT l.mint
               FROM pumphound.launches l
               WHERE l.launch_id=$1
                 AND COALESCE(l.attributes->>'integrity_state','')<>'QUARANTINED'
                 AND NOT EXISTS (
                   SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                   WHERE iq.entity_type='LAUNCH'
                     AND iq.entity_id=l.launch_id::text
                     AND iq.status='ACTIVE'
                 )"#,
        )
        .bind(launch_id)
        .fetch_optional(&stores.postgres)
        .await?
        .ok_or_else(|| anyhow::anyhow!("hot set launch does not exist or is quarantined"))?;
        if launch_mint != mint {
            tracing::warn!(
                supplied_mint = mint,
                canonical_mint = launch_mint,
                %launch_id,
                "hot set promotion corrected launch/mint identity mismatch"
            );
        }
        launch_mint
    } else {
        mint.to_string()
    };
    if canonical_mint == pumphound_domain::WSOL_MINT
        || canonical_mint == pumphound_domain::USDC_MINT
    {
        anyhow::bail!("quote assets cannot be promoted to the token Hot Set");
    }
    let mint = canonical_mint.as_str();
    let now = pumphound_domain::now_utc();
    let expires = now + chrono::Duration::from_std(HOT_SET_TTL)?;
    expire(stores, now).await?;
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pumphound.hot_set_memberships WHERE expires_at > $1 AND mint <> $2",
    )
    .bind(now)
    .bind(mint)
    .fetch_one(&stores.postgres)
    .await?;
    if existing >= HOT_SET_CAPACITY {
        evict_lowest_priority(stores).await?;
    }
    let row = sqlx::query(
        r#"INSERT INTO pumphound.hot_set_memberships
           (mint,launch_id,reason_code,promoted_by,priority,fidelity,payload,first_promoted_at,last_promoted_at,expires_at,promotion_count)
           VALUES ($1,$2,$3,$4,100,'HOT',$5,$6,$6,$7,1)
           ON CONFLICT (mint) DO UPDATE SET
             launch_id=COALESCE(EXCLUDED.launch_id, pumphound.hot_set_memberships.launch_id),
             reason_code=EXCLUDED.reason_code,
             promoted_by=EXCLUDED.promoted_by,
             last_promoted_at=EXCLUDED.last_promoted_at,
             expires_at=EXCLUDED.expires_at,
             promotion_count=pumphound.hot_set_memberships.promotion_count+1,
             payload=EXCLUDED.payload
           RETURNING mint, launch_id, reason_code, promoted_by, expires_at, promotion_count"#,
    )
    .bind(mint)
    .bind(launch_id)
    .bind(reason_code)
    .bind(promoted_by)
    .bind(json!({"ttl_seconds": HOT_SET_TTL.as_secs(), "capacity": HOT_SET_CAPACITY}))
    .bind(now)
    .bind(expires)
    .fetch_one(&stores.postgres)
    .await?;
    let count: i32 = row.get("promotion_count");
    let action = if count > 1 { "EXTENDED" } else { "PROMOTED" };
    sqlx::query(
        r#"INSERT INTO pumphound.hot_set_events
           (hot_set_event_id,mint,launch_id,action,reason_code,promoted_by,payload,occurred_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
    )
    .bind(Uuid::now_v7())
    .bind(mint)
    .bind(launch_id)
    .bind(action)
    .bind(reason_code)
    .bind(promoted_by)
    .bind(json!({"promotion_count": count}))
    .bind(now)
    .execute(&stores.postgres)
    .await?;
    tracing::info!(%mint, reason_code, promoted_by, action, "hot set membership updated");
    Ok(json!({
        "mint": mint,
        "action": action,
        "reason_code": reason_code,
        "promoted_by": promoted_by,
        "expires_at": row.get::<chrono::DateTime<chrono::Utc>,_>("expires_at"),
        "promotion_count": count
    }))
}

pub async fn active(stores: &Stores) -> anyhow::Result<Vec<HotMembership>> {
    let now = pumphound_domain::now_utc();
    expire(stores, now).await?;
    let rows = sqlx::query(
        r#"SELECT mint,launch_id,reason_code,promoted_by,expires_at
           FROM pumphound.hot_set_memberships
           WHERE expires_at > $1
           ORDER BY last_promoted_at DESC"#,
    )
    .bind(now)
    .fetch_all(&stores.postgres)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| HotMembership {
            mint: row.get("mint"),
            launch_id: row.try_get("launch_id").ok().flatten(),
            reason_code: row.get("reason_code"),
            promoted_by: row.get("promoted_by"),
            expires_at: row.get("expires_at"),
        })
        .collect())
}

pub async fn report(stores: &Stores) -> anyhow::Result<Value> {
    let members = active(stores).await?;
    let events = sqlx::query(
        r#"SELECT mint,action,reason_code,promoted_by,occurred_at
           FROM pumphound.hot_set_events ORDER BY occurred_at DESC LIMIT 25"#,
    )
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    Ok(json!({
        "capacity": HOT_SET_CAPACITY,
        "ttl_seconds": HOT_SET_TTL.as_secs(),
        "active_count": members.len(),
        "members": members.iter().map(|m| json!({
            "mint": m.mint,
            "launch_id": m.launch_id,
            "reason_code": m.reason_code,
            "promoted_by": m.promoted_by,
            "expires_at": m.expires_at,
            "fidelity": "HOT"
        })).collect::<Vec<_>>(),
        "recent_events": events.iter().map(|row| json!({
            "mint": row.get::<String,_>("mint"),
            "action": row.get::<String,_>("action"),
            "reason_code": row.get::<String,_>("reason_code"),
            "promoted_by": row.get::<String,_>("promoted_by"),
            "occurred_at": row.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at")
        })).collect::<Vec<_>>()
    }))
}

pub async fn membership_for(stores: &Stores, mint: &str) -> anyhow::Result<Option<Value>> {
    let now = pumphound_domain::now_utc();
    let row = sqlx::query(
        r#"SELECT mint,launch_id,reason_code,promoted_by,expires_at,promotion_count,fidelity
           FROM pumphound.hot_set_memberships
           WHERE mint=$1 AND expires_at > $2"#,
    )
    .bind(mint)
    .bind(now)
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(row.map(|row| {
        json!({
            "mint": row.get::<String,_>("mint"),
            "launch_id": row.try_get::<Option<Uuid>,_>("launch_id").ok().flatten(),
            "reason_code": row.get::<String,_>("reason_code"),
            "promoted_by": row.get::<String,_>("promoted_by"),
            "expires_at": row.get::<chrono::DateTime<chrono::Utc>,_>("expires_at"),
            "promotion_count": row.get::<i32,_>("promotion_count"),
            "fidelity": row.get::<String,_>("fidelity")
        })
    }))
}

async fn expire(stores: &Stores, now: chrono::DateTime<chrono::Utc>) -> anyhow::Result<()> {
    let expired = sqlx::query(
        r#"DELETE FROM pumphound.hot_set_memberships h
           WHERE expires_at <= $1 OR mint IN ($2,$3)
              OR EXISTS (
                SELECT 1 FROM pumphound.launches l
                WHERE l.launch_id=h.launch_id
                  AND (
                    COALESCE(l.attributes->>'integrity_state','')='QUARANTINED'
                    OR EXISTS (
                      SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                      WHERE iq.entity_type='LAUNCH'
                        AND iq.entity_id=l.launch_id::text
                        AND iq.status='ACTIVE'
                    )
                  )
              )
           RETURNING mint, launch_id, reason_code, promoted_by"#,
    )
    .bind(now)
    .bind(pumphound_domain::WSOL_MINT)
    .bind(pumphound_domain::USDC_MINT)
    .fetch_all(&stores.postgres)
    .await?;
    for row in expired {
        let mint = row.get::<String, _>("mint");
        let invalid_quote_asset =
            mint == pumphound_domain::WSOL_MINT || mint == pumphound_domain::USDC_MINT;
        sqlx::query(
            r#"INSERT INTO pumphound.hot_set_events
               (hot_set_event_id,mint,launch_id,action,reason_code,promoted_by,payload,occurred_at)
               VALUES ($1,$2,$3,'EXPIRED',$4,$5,$6,$7)"#,
        )
        .bind(Uuid::now_v7())
        .bind(&mint)
        .bind(row.try_get::<Option<Uuid>, _>("launch_id").ok().flatten())
        .bind(row.get::<String, _>("reason_code"))
        .bind(row.get::<String, _>("promoted_by"))
        .bind(if invalid_quote_asset {
            json!({"reason": "invalid_quote_asset_membership"})
        } else {
            json!({})
        })
        .bind(now)
        .execute(&stores.postgres)
        .await?;
    }
    Ok(())
}

async fn evict_lowest_priority(stores: &Stores) -> anyhow::Result<()> {
    let row = sqlx::query(
        r#"DELETE FROM pumphound.hot_set_memberships
           WHERE mint = (
             SELECT mint FROM pumphound.hot_set_memberships
             ORDER BY priority ASC, last_promoted_at ASC LIMIT 1
           )
           RETURNING mint, launch_id, reason_code, promoted_by"#,
    )
    .fetch_optional(&stores.postgres)
    .await?;
    if let Some(row) = row {
        sqlx::query(
            r#"INSERT INTO pumphound.hot_set_events
               (hot_set_event_id,mint,launch_id,action,reason_code,promoted_by,payload,occurred_at)
               VALUES ($1,$2,$3,'EVICTED',$4,$5,'{"reason":"capacity"}'::jsonb,now())"#,
        )
        .bind(Uuid::now_v7())
        .bind(row.get::<String, _>("mint"))
        .bind(row.try_get::<Option<Uuid>, _>("launch_id").ok().flatten())
        .bind(row.get::<String, _>("reason_code"))
        .bind(row.get::<String, _>("promoted_by"))
        .execute(&stores.postgres)
        .await?;
    }
    Ok(())
}
