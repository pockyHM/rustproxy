use anyhow::{Context, Result};
use std::fs;

use crate::config::yaml::AppConfig;
use crate::db::{migration, Database};

pub fn run_export(db_path: &str) -> Result<()> {
    let db = Database::open(db_path)?;
    let yaml = migration::export_yaml(&db)?;
    print!("{yaml}");
    Ok(())
}

pub fn run_import(db_path: &str, file_path: &str, replace: bool) -> Result<()> {
    let db = Database::open(db_path)?;

    if !replace && !db.is_empty()? {
        anyhow::bail!("database is not empty. Use --replace to overwrite existing configuration.");
    }

    let config = AppConfig::load(file_path)
        .with_context(|| format!("failed to load YAML from: {file_path}"))?;
    migration::import_yaml(&db, &config)?;
    println!("Configuration imported from {file_path}");
    Ok(())
}

pub fn run_edit(db_path: &str) -> Result<()> {
    let db = Database::open(db_path)?;

    // Export current config to a temp YAML file
    let yaml = migration::export_yaml(&db)?;
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("rustproxy-config-edit.yaml");
    fs::write(&temp_path, &yaml)
        .with_context(|| "failed to write temp config file")?;

    let original_metadata = fs::metadata(&temp_path)?;

    // Open $EDITOR
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor)
        .arg(&temp_path)
        .status()
        .with_context(|| format!("failed to launch editor: {editor}"))?;

    if !status.success() {
        anyhow::bail!("editor exited with non-zero status");
    }

    // Check if file was modified
    let new_metadata = fs::metadata(&temp_path)?;
    if let (Ok(original), Ok(new)) = (original_metadata.modified(), new_metadata.modified()) {
        if new <= original {
            println!("No changes detected.");
            let _ = fs::remove_file(&temp_path);
            return Ok(());
        }
    }

    // Validate and reimport
    let content = fs::read_to_string(&temp_path)?;
    let config: AppConfig = serde_yaml::from_str(&content)
        .with_context(|| "YAML validation failed".to_string())?;
    migration::import_yaml(&db, &config)?;
    println!("Configuration updated.");

    let _ = fs::remove_file(&temp_path);
    Ok(())
}
