use std::sync::Arc;

use flaresolverr_rs::config::{load, FlareSolverConfig};
use flaresolverr_rs::router::create_router;
use flaresolverr_rs::session::SessionStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg: FlareSolverConfig = load().map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&cfg.log_level)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let store = Arc::new(SessionStore::new(cfg.headless, cfg.no_sandbox));
    store.init_ephemeral().await;
    let router = create_router(store);
    let addr = format!("{}:{}", cfg.host, cfg.port);

    tracing::info!("flaresolverr-rs starting on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
