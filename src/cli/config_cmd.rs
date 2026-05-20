use anyhow::{Context, Result};
use std::fs;

use crate::cli::{RuleCommands, UpstreamCommands};
use crate::config::yaml::{AccessLogLevel, AppConfig, Fallback};
use crate::db::{migration, Database};
use crate::models::{
    ConditionExpr, ConditionType, HostMatchType, HostMatcher, LocationMatchType, LocationMatcher,
    Operator, Rule, Target, Upstream,
};

const CONFIG_KEYS: &[&str] = &[
    "listen",
    "proxy_listen",
    "fallback_url",
    "connect_timeout",
    "request_timeout",
    "pool_max_idle_per_host",
    "pool_idle_timeout",
    "tcp_keepalive",
    "access_log_enabled",
    "access_log_path",
    "access_log_buffer_size",
    "access_log_level",
];

pub fn run_get(db_path: &str, key: Option<&str>) -> Result<()> {
    let db = Database::open(db_path)?;
    let config = db.load_config()?;

    match key {
        Some(key) => {
            println!("{}", get_config_value(&config, key)?);
        }
        None => {
            for key in CONFIG_KEYS {
                println!("{key}={}", get_config_value(&config, key)?);
            }
        }
    }

    Ok(())
}

pub fn run_set(db_path: &str, key: &str, value: &str) -> Result<()> {
    let db = Database::open(db_path)?;
    let mut config = db.load_config()?;
    ensure_minimum_config(&mut config);
    set_config_value(&mut config, key, value)?;
    db.save_full_config(&config)?;
    println!("{key}={}", get_config_value(&config, key)?);
    Ok(())
}

pub fn run_upstream(db_path: &str, command: UpstreamCommands) -> Result<()> {
    let db = Database::open(db_path)?;
    let mut config = db.load_config()?;
    ensure_minimum_config(&mut config);

    match command {
        UpstreamCommands::List => {
            for upstream in config.upstreams.values() {
                println!("{}:", upstream.name);
                for target in &upstream.targets {
                    println!("  {} weight={}", target.url, target.weight);
                }
            }
            return Ok(());
        }
        UpstreamCommands::Add { name, url, weight } => {
            validate_name("upstream", &name)?;
            validate_upstream_url(&url)?;
            if config.upstreams.contains_key(&name) {
                anyhow::bail!("upstream '{name}' already exists");
            }
            config.upstreams.insert(
                name.clone(),
                Upstream {
                    name: name.clone(),
                    skip_ssl: false,
                    websocket: false,
                    targets: vec![Target { url, weight }],
                    health_check: Default::default(),
                    balance: Default::default(),
                    retry: Default::default(),
                },
            );
            db.save_full_config(&config)?;
            println!("upstream {name} added");
        }
        UpstreamCommands::AddTarget { name, url, weight } => {
            validate_upstream_url(&url)?;
            let Some(upstream) = config.upstreams.get_mut(&name) else {
                anyhow::bail!("upstream '{name}' not found");
            };
            upstream.targets.push(Target { url, weight });
            db.save_full_config(&config)?;
            println!("target added to upstream {name}");
        }
        UpstreamCommands::Delete { name } => {
            if config.rules.iter().any(|rule| rule.upstream == name) {
                anyhow::bail!("upstream '{name}' is still referenced by at least one rule");
            }
            if config.upstreams.remove(&name).is_none() {
                anyhow::bail!("upstream '{name}' not found");
            }
            db.save_full_config(&config)?;
            println!("upstream {name} deleted");
        }
    }

    Ok(())
}

pub fn run_rule(db_path: &str, command: RuleCommands) -> Result<()> {
    let db = Database::open(db_path)?;
    let mut config = db.load_config()?;
    ensure_minimum_config(&mut config);

    match command {
        RuleCommands::List => {
            for rule in &config.rules {
                let listen = rule.listen.as_deref().unwrap_or("default");
                println!(
                    "{} priority={} upstream={} listen={} name={}",
                    rule.id, rule.priority, rule.upstream, listen, rule.name
                );
            }
            return Ok(());
        }
        RuleCommands::Add {
            id,
            name,
            upstream,
            priority,
            weight,
            listen,
            host_type,
            host,
            location_type,
            location,
            condition_type,
            operator,
            value,
            key,
            claim_path,
        } => {
            validate_name("rule", &id)?;
            if config.rules.iter().any(|rule| rule.id == id) {
                anyhow::bail!("rule '{id}' already exists");
            }
            if !config.upstreams.contains_key(&upstream) {
                anyhow::bail!("upstream '{upstream}' not found");
            }
            if let Some(listen) = listen.as_deref() {
                validate_listen_addr(listen)?;
            }

            let conditions = build_condition(condition_type, operator, value, key, claim_path)?;
            let host = build_host_matcher(&host_type, host)?;
            let location = build_location_matcher(&location_type, location)?;
            config.rules.push(Rule {
                id: id.clone(),
                name,
                priority,
                host,
                location,
                match_set: None,
                conditions,
                upstream,
                weight,
                is_fallback: false,
                listen,
                request_timeout: 0,
                tls: None,
                header_policy: Default::default(),
                path_actions: Vec::new(),
                limit_policy: Default::default(),
            });
            config
                .rules
                .sort_by_key(|rule| std::cmp::Reverse(rule.priority));
            db.save_full_config(&config)?;
            println!("rule {id} added");
        }
        RuleCommands::Delete { id } => {
            let before = config.rules.len();
            config.rules.retain(|rule| rule.id != id);
            if config.rules.len() == before {
                anyhow::bail!("rule '{id}' not found");
            }
            db.save_full_config(&config)?;
            println!("rule {id} deleted");
        }
    }

    Ok(())
}

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
    let mut config = config;
    config.normalize_rules();
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
    fs::write(&temp_path, &yaml).with_context(|| "failed to write temp config file")?;

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
    let config: AppConfig =
        serde_yaml::from_str(&content).with_context(|| "YAML validation failed".to_string())?;
    migration::import_yaml(&db, &config)?;
    println!("Configuration updated.");

    let _ = fs::remove_file(&temp_path);
    Ok(())
}

fn ensure_minimum_config(config: &mut AppConfig) {
    if config.listen.is_empty() {
        config.listen = "0.0.0.0:3000".to_string();
    }
    if config.proxy_listen.is_empty() {
        config.proxy_listen = "0.0.0.0:80".to_string();
    }
    if config.fallback.url.is_empty() {
        config.fallback = Fallback {
            url: "404".to_string(),
        };
    }
    config.normalize_rules();
}

fn get_config_value(config: &AppConfig, key: &str) -> Result<String> {
    match key {
        "listen" => Ok(config.listen.clone()),
        "proxy_listen" => Ok(config.proxy_listen.clone()),
        "fallback_url" => Ok(config.fallback.url.clone()),
        "connect_timeout" => Ok(config.connect_timeout.to_string()),
        "request_timeout" => Ok(config.request_timeout.to_string()),
        "pool_max_idle_per_host" => Ok(config.pool_max_idle_per_host.to_string()),
        "pool_idle_timeout" => Ok(config.pool_idle_timeout.to_string()),
        "tcp_keepalive" => Ok(config.tcp_keepalive.to_string()),
        "access_log_enabled" => Ok(config.access_log.enabled.to_string()),
        "access_log_path" => Ok(config.access_log.path.clone().unwrap_or_default()),
        "access_log_buffer_size" => Ok(config.access_log.buffer_size.unwrap_or(8192).to_string()),
        "access_log_level" => Ok(config.access_log.level.as_str().to_string()),
        _ => unknown_key(key),
    }
}

fn set_config_value(config: &mut AppConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "listen" => {
            validate_listen_addr(value)?;
            config.listen = value.to_string();
        }
        "proxy_listen" => {
            validate_listen_addr(value)?;
            config.proxy_listen = value.to_string();
        }
        "fallback_url" => {
            validate_upstream_url(value)?;
            config.fallback.url = value.to_string();
        }
        "connect_timeout" => config.connect_timeout = parse_u64(key, value)?,
        "request_timeout" => config.request_timeout = parse_u64(key, value)?,
        "pool_max_idle_per_host" => config.pool_max_idle_per_host = parse_usize(key, value)?,
        "pool_idle_timeout" => config.pool_idle_timeout = parse_u64(key, value)?,
        "tcp_keepalive" => config.tcp_keepalive = parse_u64(key, value)?,
        "access_log_enabled" => config.access_log.enabled = parse_bool(key, value)?,
        "access_log_path" => {
            config.access_log.path = if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "access_log_buffer_size" => config.access_log.buffer_size = Some(parse_usize(key, value)?),
        "access_log_level" => config.access_log.level = parse_access_log_level(value)?,
        _ => unknown_key(key)?,
    }
    Ok(())
}

fn parse_access_log_level(value: &str) -> Result<AccessLogLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "debug" => Ok(AccessLogLevel::Debug),
        "info" => Ok(AccessLogLevel::Info),
        "warn" | "warning" => Ok(AccessLogLevel::Warn),
        "error" => Ok(AccessLogLevel::Error),
        _ => anyhow::bail!("access_log_level must be one of debug, info, warn, error"),
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    value
        .parse()
        .with_context(|| format!("{key} must be true or false"))
}

fn parse_u64(key: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .with_context(|| format!("{key} must be a non-negative integer"))
}

fn parse_usize(key: &str, value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .with_context(|| format!("{key} must be a non-negative integer"))
}

fn validate_listen_addr(value: &str) -> Result<()> {
    let Some((_, port)) = value.rsplit_once(':') else {
        anyhow::bail!("listen address must include a port, for example 0.0.0.0:3000");
    };
    port.parse::<u16>()
        .with_context(|| format!("invalid listen port in address: {value}"))?;
    Ok(())
}

fn validate_upstream_url(value: &str) -> Result<()> {
    if value == "404" {
        return Ok(());
    }
    let uri = value
        .parse::<http::Uri>()
        .with_context(|| format!("invalid URL: {value}"))?;
    match uri.scheme_str() {
        Some("http") | Some("https") => Ok(()),
        _ => anyhow::bail!("fallback_url must use http://, https://, or 404"),
    }
}

fn validate_name(kind: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{kind} name cannot be empty");
    }
    Ok(())
}

fn build_condition(
    condition_type: Option<String>,
    operator: Option<String>,
    value: Option<String>,
    key: Option<String>,
    claim_path: Option<String>,
) -> Result<Option<ConditionExpr>> {
    let Some(condition_type) = condition_type else {
        if operator.is_some() || value.is_some() || key.is_some() || claim_path.is_some() {
            anyhow::bail!("--condition-type is required when condition options are provided");
        }
        return Ok(None);
    };

    let condition_type = parse_condition_type(&condition_type)?;
    let operator = parse_operator(operator.as_deref().unwrap_or("exists"))?;

    match condition_type {
        ConditionType::Header | ConditionType::Cookie => {
            if key.as_deref().unwrap_or("").is_empty() {
                anyhow::bail!("--key is required for header and cookie conditions");
            }
        }
        ConditionType::Jwt => {
            if claim_path.as_deref().unwrap_or("").is_empty() {
                anyhow::bail!("--claim-path is required for jwt conditions");
            }
        }
        ConditionType::Host | ConditionType::Path => {
            anyhow::bail!("host and path must be configured with --host-* and --location-* options")
        }
    }

    if operator != Operator::Exists && value.is_none() {
        anyhow::bail!("--value is required unless --operator exists is used");
    }

    Ok(Some(ConditionExpr::Leaf {
        condition_type,
        key,
        claim_path,
        operator,
        value,
    }))
}

fn build_host_matcher(host_type: &str, value: Option<String>) -> Result<HostMatcher> {
    let match_type = match host_type.to_ascii_lowercase().as_str() {
        "any" => HostMatchType::Any,
        "exact" => HostMatchType::Exact,
        "wildcard" => HostMatchType::Wildcard,
        _ => anyhow::bail!("host type must be one of: any, exact, wildcard"),
    };
    let value = match match_type {
        HostMatchType::Any => None,
        HostMatchType::Exact => Some(
            value
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("--host is required for exact host matching"))?,
        ),
        HostMatchType::Wildcard => {
            let value = value
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("--host is required for wildcard host matching"))?;
            if !value.starts_with("*.") {
                anyhow::bail!("wildcard host must use '*.example.com' format");
            }
            Some(value)
        }
    };
    Ok(HostMatcher { match_type, value })
}

fn build_location_matcher(location_type: &str, value: String) -> Result<LocationMatcher> {
    let match_type = match location_type.to_ascii_lowercase().as_str() {
        "exact" => LocationMatchType::Exact,
        "prefix" => LocationMatchType::Prefix,
        "regex" => LocationMatchType::Regex,
        _ => anyhow::bail!("location type must be one of: exact, prefix, regex"),
    };
    if !matches!(match_type, LocationMatchType::Regex) && !value.starts_with('/') {
        anyhow::bail!("location must start with '/'");
    }
    if matches!(match_type, LocationMatchType::Regex) {
        regex::Regex::new(&value).with_context(|| "invalid location regex")?;
    }
    Ok(LocationMatcher { match_type, value })
}

fn parse_condition_type(value: &str) -> Result<ConditionType> {
    match value.to_ascii_lowercase().as_str() {
        "header" => Ok(ConditionType::Header),
        "cookie" => Ok(ConditionType::Cookie),
        "jwt" => Ok(ConditionType::Jwt),
        _ => anyhow::bail!("condition type must be one of: header, cookie, jwt"),
    }
}

fn parse_operator(value: &str) -> Result<Operator> {
    match value.to_ascii_lowercase().as_str() {
        "exact" => Ok(Operator::Exact),
        "prefix" => Ok(Operator::Prefix),
        "regex" => Ok(Operator::Regex),
        "exists" => Ok(Operator::Exists),
        "contains" => Ok(Operator::Contains),
        _ => anyhow::bail!("operator must be one of: exact, prefix, regex, exists, contains"),
    }
}

fn unknown_key<T>(key: &str) -> Result<T> {
    anyhow::bail!(
        "unknown config key '{key}'. Supported keys: {}",
        CONFIG_KEYS.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config() -> AppConfig {
        AppConfig {
            listen: "0.0.0.0:3000".to_string(),
            proxy_listen: "0.0.0.0:80".to_string(),
            timeouts: Default::default(),
            limits: Default::default(),
            connect_timeout: 10,
            request_timeout: 60,
            pool_max_idle_per_host: 32,
            pool_idle_timeout: 90,
            tcp_keepalive: 60,
            certificate_dir: "/etc/rustproxy/cert.d".to_string(),
            access_log: Default::default(),
            monitoring: Default::default(),
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: vec![],
            upstreams: HashMap::new(),
            fallback: Fallback {
                url: "http://fallback.local".to_string(),
            },
        }
    }

    #[test]
    fn set_config_value_updates_supported_keys() {
        let mut config = config();

        set_config_value(&mut config, "listen", "127.0.0.1:4000").unwrap();
        set_config_value(&mut config, "proxy_listen", "0.0.0.0:8080").unwrap();
        set_config_value(&mut config, "fallback_url", "https://fallback.local").unwrap();
        set_config_value(&mut config, "connect_timeout", "2").unwrap();
        set_config_value(&mut config, "request_timeout", "30").unwrap();
        set_config_value(&mut config, "pool_max_idle_per_host", "128").unwrap();
        set_config_value(&mut config, "pool_idle_timeout", "45").unwrap();
        set_config_value(&mut config, "tcp_keepalive", "20").unwrap();
        set_config_value(&mut config, "access_log_enabled", "true").unwrap();
        set_config_value(&mut config, "access_log_path", "/tmp/rustproxy-access.log").unwrap();
        set_config_value(&mut config, "access_log_buffer_size", "2048").unwrap();
        set_config_value(&mut config, "access_log_level", "warn").unwrap();

        assert_eq!(config.listen, "127.0.0.1:4000");
        assert_eq!(config.proxy_listen, "0.0.0.0:8080");
        assert_eq!(config.fallback.url, "https://fallback.local");
        assert_eq!(config.connect_timeout, 2);
        assert_eq!(config.request_timeout, 30);
        assert_eq!(config.pool_max_idle_per_host, 128);
        assert_eq!(config.pool_idle_timeout, 45);
        assert_eq!(config.tcp_keepalive, 20);
        assert!(config.access_log.enabled);
        assert_eq!(
            config.access_log.path.as_deref(),
            Some("/tmp/rustproxy-access.log")
        );
        assert_eq!(config.access_log.buffer_size, Some(2048));
        assert_eq!(config.access_log.level, AccessLogLevel::Warn);
    }

    #[test]
    fn rejects_invalid_config_values() {
        let mut config = config();

        assert!(set_config_value(&mut config, "listen", "127.0.0.1").is_err());
        assert!(set_config_value(&mut config, "fallback_url", "ftp://fallback.local").is_err());
        assert!(set_config_value(&mut config, "connect_timeout", "-1").is_err());
        assert!(set_config_value(&mut config, "access_log_level", "verbose").is_err());
        assert!(set_config_value(&mut config, "missing", "value").is_err());
    }
}
