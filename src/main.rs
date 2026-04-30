use clap::Parser;
use rummage::config::Config;
use rummage::db;
use rummage::server;
use std::process;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("No .env file found: {}", e);
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut config = Config::parse();

    // Fallback to the standard NOTMUCH_CONFIG env var used by the notmuch CLI
    // and by notmore. This lets Docker images and shell environments set a
    // single variable that every tool in the ecosystem understands.
    if config.notmuch_config.is_none() {
        if let Ok(path) = std::env::var("NOTMUCH_CONFIG") {
            config.notmuch_config = Some(path.into());
        }
    }

    if let Err(e) = run(config).await {
        error!("Fatal error: {}", e);
        process::exit(1);
    }
}

async fn run(config: Config) -> anyhow::Result<()> {
    info!("Rummage starting — maildir: {}", config.maildir.display());

    let db_path = config.notmuch_config.as_deref();

    if config.index {
        info!("Running full re-index (--index)");
        let maildir = config.maildir.clone();
        let db_path = db_path.map(std::borrow::ToOwned::to_owned);
        let start = std::time::Instant::now();
        tokio::task::spawn_blocking(move || db::force_reindex(&maildir, db_path.as_deref()))
            .await??;
        info!(
            "Indexing complete in {:.1}s. Exiting.",
            start.elapsed().as_secs_f64()
        );
        return Ok(());
    }

    info!("Spawning database worker...");
    let db_handle =
        db::spawn_database_worker(&config.maildir, db_path, config.no_auto_index).await?;
    info!("Database worker ready");

    let mcp_enabled = !config.no_mcp
        && std::env::var("RUMMAGE_MCP_ENABLED")
            .ok()
            .map(|v| v != "false")
            .unwrap_or(true);
    let webui_enabled = !config.no_webui;

    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("Failed to install Prometheus metrics recorder: {e}"))?;
    server::set_metrics_handle(prometheus_handle);

    info!(
        "Starting server on http://{}:{} (local only)",
        config.host, config.port
    );

    let router_config = server::RouterConfig {
        webui_enabled,
        mcp_enabled,
        mcp_path: config.mcp_path.clone(),
    };

    server::serve(db_handle, &config, router_config).await?;

    Ok(())
}
