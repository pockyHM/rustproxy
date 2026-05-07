use anyhow::Result;
use rustproxy::{api::server, config::AppConfig};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.yaml".to_string());
    let config = AppConfig::load(&config_path)?;

    tracing::info!(%config_path, "Starting rustproxy server");
    server::run(config, config_path).await
}

