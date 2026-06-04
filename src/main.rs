use std::sync::Arc;

use chaser_cf::ChaserCF;
use flaresolverr_rs::config::load;
use flaresolverr_rs::router::create_router;
use flaresolverr_rs::session::SessionStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = load().map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&cfg.log_level)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    tracing::info!("initializing chaser-cf browser engine");
    let chaser = Arc::new(ChaserCF::new(cfg.to_chaser_config()).await?);
    let store = Arc::new(SessionStore::new(chaser));
    let router = create_router(store);
    let addr = format!("{}:{}", cfg.host, cfg.port);

    tracing::info!("flaresolverr-rs listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
