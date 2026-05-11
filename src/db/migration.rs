use anyhow::{Context, Result};

use crate::config::yaml::AppConfig;
use crate::db::Database;

pub fn import_yaml(db: &Database, config: &AppConfig) -> Result<()> {
    db.save_full_config(config)
        .context("failed to import YAML config into database")
}

pub fn export_yaml(db: &Database) -> Result<String> {
    let config = db.load_config()?;
    serde_yaml::to_string(&config).context("failed to serialize config to YAML")
}

pub fn import_yaml_file(db: &Database, path: &str) -> Result<()> {
    let config = AppConfig::load(path)?;
    import_yaml(db, &config)
}
