pub mod config_cmd;
pub mod serve;
pub mod user_cmd;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rustproxy")]
#[command(about = "High-performance Rust traffic routing proxy")]
#[command(version)]
pub struct Cli {
    /// Path to SQLite database file
    #[arg(long, global = true, default_value = "rustproxy.db", env = "RUSTPROXY_DB")]
    pub db: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the proxy server
    Serve {
        /// Optional YAML config to import on first run (when DB is empty)
        #[arg(default_value = "config.yaml")]
        config: Option<String>,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Manage admin users
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Export database config to YAML on stdout
    Export,

    /// Import YAML configuration file into the database
    Import {
        /// Path to YAML configuration file
        file: String,

        /// Replace entire config (clears existing rules and upstreams)
        #[arg(long)]
        replace: bool,
    },

    /// Open config in $EDITOR for editing (exports to temp YAML, reimports on save)
    Edit,
}

#[derive(Subcommand)]
pub enum UserCommands {
    /// Add a new admin user (prompts for password)
    Add {
        /// Username for the new admin
        username: String,
    },

    /// List all admin users
    List,

    /// Change password for an admin user (prompts for new password)
    Passwd {
        /// Username to change password for
        username: String,
    },
}
