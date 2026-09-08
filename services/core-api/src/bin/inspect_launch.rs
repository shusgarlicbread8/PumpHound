use anyhow::Context;
use pumphound_storage::Stores;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mint = std::env::args()
        .nth(1)
        .expect("usage: inspect-launch <mint>");
    let stores = Stores::connect().await.context("stores")?;
    let row = sqlx::query(
        "SELECT launch_id::text, mint, protocol, creator, quote_mint, creation_slot, creation_signature, canonicality
         FROM pumphound.launches WHERE mint=$1 LIMIT 1",
    )
    .bind(&mint)
    .fetch_optional(&stores.postgres)
    .await?;
    match row {
        Some(r) => {
            use sqlx::Row;
            println!("launch_id={}", r.get::<String, _>("launch_id"));
            println!("mint={}", r.get::<String, _>("mint"));
            println!("protocol={}", r.get::<String, _>("protocol"));
            println!("creator={}", r.get::<String, _>("creator"));
            println!("quote_mint={}", r.get::<String, _>("quote_mint"));
            println!("creation_slot={}", r.get::<i64, _>("creation_slot"));
            println!("signature={}", r.get::<String, _>("creation_signature"));
            println!("canonicality={}", r.get::<String, _>("canonicality"));
            println!("quality=inspect via /api/v1/launches/{}", mint);
        }
        None => {
            println!("unknown mint {mint}");
            std::process::exit(2);
        }
    }
    Ok(())
}
