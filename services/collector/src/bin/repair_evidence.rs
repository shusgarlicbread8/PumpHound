#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,pumphound_collector=info".into()),
        )
        .init();
    let mut config = pumphound_collector::evidence::RepairConfig::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(only) = args.iter().position(|arg| arg == "--only") {
        let phases = args.get(only + 1).map(String::as_str).unwrap_or("");
        config.launch_times = false;
        config.identities = false;
        config.pumpswap = false;
        config.mint_backfill = false;
        config.snapshots = false;
        config.import_corpus = false;
        for phase in phases.split(',') {
            match phase.trim() {
                "launch-times" => config.launch_times = true,
                "interpolate" => {
                    config.launch_times = true;
                    config.skip_block_time_rpc = true;
                }
                "identities" => config.identities = true,
                "pumpswap" => config.pumpswap = true,
                "mint-backfill" => config.mint_backfill = true,
                "snapshots" => config.snapshots = true,
                "import-corpus" => config.import_corpus = true,
                other => anyhow::bail!("unknown repair phase {other}"),
            }
        }
        config.identities = true;
    }
    if let Some(dir) = args.iter().position(|arg| arg == "--corpus") {
        config.corpus_dir = args.get(dir + 1).cloned();
    }
    let stores = pumphound_storage::Stores::connect().await?;
    let report = pumphound_collector::evidence::run(&stores, config).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
