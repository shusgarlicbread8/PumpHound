use pumphound_chain_source::SourceConfig;
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = SourceConfig::from_env();
    println!("{}", cfg.label());
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSlot",
        "params": [{"commitment": "processed"}]
    });
    let res = reqwest::Client::new()
        .post(&cfg.rpc_url)
        .json(&body)
        .send()
        .await?;
    let v: serde_json::Value = res.json().await?;
    println!(
        "rpc_slot={}",
        v.get("result").cloned().unwrap_or(json!(null))
    );
    Ok(())
}
