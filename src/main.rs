use anyhow::Result;
use clap::Parser;
use rustproxy::cli::{Cli, Commands, ConfigCommands, UserCommands};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config } => {
            let yaml_path = config.unwrap_or_else(|| "config.yaml".to_string());
            rustproxy::cli::serve::run(&cli.db, &yaml_path).await
        }
        Commands::Config { command } => match command {
            ConfigCommands::Export => {
                rustproxy::cli::config_cmd::run_export(&cli.db)
            }
            ConfigCommands::Import { file, replace } => {
                rustproxy::cli::config_cmd::run_import(&cli.db, &file, replace)
            }
            ConfigCommands::Edit => {
                rustproxy::cli::config_cmd::run_edit(&cli.db)
            }
        },
        Commands::User { command } => match command {
            UserCommands::Add { username } => {
                rustproxy::cli::user_cmd::run_add(&cli.db, &username)
            }
            UserCommands::List => {
                rustproxy::cli::user_cmd::run_list(&cli.db)
            }
            UserCommands::Passwd { username } => {
                rustproxy::cli::user_cmd::run_passwd(&cli.db, &username)
            }
        },
    }
}
