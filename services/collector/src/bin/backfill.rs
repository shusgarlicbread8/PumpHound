#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,pumphound_collector=info".into()),
        )
        .init();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (operation, config_path) = match args.as_slice() {
        [path] => ("backfill", path.as_str()),
        [flag, path] if flag == "--estimate" => ("estimate", path.as_str()),
        [flag, path] if flag == "--redecode" => ("redecode", path.as_str()),
        _ => panic!("usage: backfill [--estimate|--redecode] <history-config.json>"),
    };
    let (config, config_sha256) =
        pumphound_collector::history::HistoryConfig::load(std::path::Path::new(config_path))?;
    if operation == "estimate" {
        println!(
            "windows={} target_launches={} max_raw_transactions={} max_requests={} conservative_max_credits={}",
            config.windows.len(),
            config.target_launches,
            config.max_raw_transactions,
            config.max_requests,
            config.maximum_credit_estimate()
        );
        return Ok(());
    }
    let manifest = if operation == "redecode" {
        pumphound_collector::history::redecode(config, config_sha256).await?
    } else {
        pumphound_collector::history::run(config, config_sha256).await?
    };
    pumphound_collector::history::ensure_quality(&manifest)?;
    println!(
        "history backfill complete mode={:?} raw={} canonical={} unique_launches={} failures={} fallback={:?} manifest={}",
        manifest.effective_mode,
        manifest.raw_count,
        manifest.canonical_event_count,
        manifest.unique_launch_observations,
        manifest.failure_count,
        manifest.fallback_reason,
        std::path::Path::new(&manifest.canonical_events_uri)
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("corpus-manifest.json")
            .display()
    );
    Ok(())
}
