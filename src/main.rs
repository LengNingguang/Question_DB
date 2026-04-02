use anyhow::Result;
use qb_api::{
    api::{router, AppState},
    config::AppConfig,
    db::create_pool,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = AppConfig::from_env()?;
    let pool = create_pool(&cfg.database_url).await?;

    let app = router(AppState { pool });
    let listener = TcpListener::bind(cfg.bind_addr).await?;

    tracing::info!(addr = %cfg.bind_addr, "qb_api_rust listening");
    axum::serve(listener, app).await?;
    Ok(())
}
