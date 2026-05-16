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
    #[arg(
        long,
        global = true,
        default_value = "rustproxy.db",
        env = "RUSTPROXY_DB"
    )]
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
    /// Print current configuration, or one top-level config value
    Get {
        /// Config key to print (version, listen, proxy_listen, fallback_url, connect_timeout, request_timeout, pool_max_idle_per_host, pool_idle_timeout, tcp_keepalive)
        key: Option<String>,
    },

    /// Set one top-level config value in the database
    Set {
        /// Config key to update
        key: String,

        /// New value for the config key
        value: String,
    },

    /// Manage upstream pools
    Upstream {
        #[command(subcommand)]
        command: UpstreamCommands,
    },

    /// Manage routing rules
    Rule {
        #[command(subcommand)]
        command: RuleCommands,
    },

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
pub enum UpstreamCommands {
    /// List upstream pools
    List,

    /// Add an upstream pool with one target
    Add {
        /// Upstream name
        name: String,

        /// Target URL, for example http://127.0.0.1:8080
        url: String,

        /// Target weight
        #[arg(long, default_value_t = 100)]
        weight: u32,
    },

    /// Add a target to an existing upstream pool
    AddTarget {
        /// Upstream name
        name: String,

        /// Target URL, for example http://127.0.0.1:8080
        url: String,

        /// Target weight
        #[arg(long, default_value_t = 100)]
        weight: u32,
    },

    /// Delete an upstream pool
    Delete {
        /// Upstream name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum RuleCommands {
    /// List routing rules
    List,

    /// Add a routing rule. Omit condition options for an always-match rule.
    Add {
        /// Rule id
        id: String,

        /// Human-readable rule name
        #[arg(long)]
        name: String,

        /// Upstream pool name
        #[arg(long)]
        upstream: String,

        /// Rule priority. Higher priority matches first.
        #[arg(long, default_value_t = 0)]
        priority: i32,

        /// Rule weight metadata
        #[arg(long, default_value_t = 100)]
        weight: u32,

        /// Dedicated listen address for this rule, for example 0.0.0.0:9090
        #[arg(long)]
        listen: Option<String>,

        /// Host match type: any, exact, wildcard
        #[arg(long, default_value = "any")]
        host_type: String,

        /// Host value for exact/wildcard host matching
        #[arg(long)]
        host: Option<String>,

        /// Location match type: exact, prefix, regex
        #[arg(long, default_value = "prefix")]
        location_type: String,

        /// Location value
        #[arg(long, default_value = "/")]
        location: String,

        /// Condition type: header, cookie, jwt
        #[arg(long)]
        condition_type: Option<String>,

        /// Condition operator: exact, prefix, regex, exists, contains
        #[arg(long)]
        operator: Option<String>,

        /// Condition value. Not required for exists.
        #[arg(long)]
        value: Option<String>,

        /// Header or cookie name for header/cookie conditions
        #[arg(long)]
        key: Option<String>,

        /// JWT claim path for jwt conditions, for example user.role
        #[arg(long)]
        claim_path: Option<String>,
    },

    /// Delete a routing rule
    Delete {
        /// Rule id
        id: String,
    },
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
