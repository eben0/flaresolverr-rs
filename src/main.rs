use std::sync::Arc;

use cain_core::config::load;
use flare_solver::config::FlareSolverConfig;
use flare_solver::router::create_router;
use flare_solver::session::SessionStore;

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

    let store = Arc::new(SessionStore::new(cfg.headless));
    let router = create_router(store);
    let addr = format!("{}:{}", cfg.host, cfg.port);

    tracing::info!("flare_solver starting on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
