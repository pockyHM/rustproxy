use anyhow::{Context, Result};

use crate::api::routes as server;
use crate::config::yaml::AppConfig;
use crate::db::{migration, Database};

pub async fn run(db_path: &str, yaml_path: &str) -> Result<()> {
    let db =
        Database::open(db_path).with_context(|| format!("failed to open database: {db_path}"))?;

    // Import YAML on first run if DB is empty
    if db.is_empty()? {
        match AppConfig::load(yaml_path) {
            Ok(config) => {
                tracing::info!(path = yaml_path, "importing configuration from YAML");
                migration::import_yaml(&db, &config)?;
            }
            Err(_) => {
                tracing::info!("no existing YAML config found, starting with empty configuration");
            }
        }
    }

    db.ensure_jwt_secret()?;

    let users = db.list_users()?;
    if users.is_empty() {
        tracing::warn!(
            "no admin users configured. run `rustproxy user add <username>` to create one."
        );
    }

    let config = db.load_config()?;
    let shutdown = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => tracing::info!("shutdown signal received"),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "signal handling unavailable; continuing without graceful signal shutdown"
                );
                std::future::pending::<()>().await;
            }
        }
    };
    server::run_until_shutdown(config, db, shutdown).await
}
