use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::config::yaml::{
    AccessLogConfig, AppConfig, Certificate, Fallback, MonitoringConfig, TlsListener,
};
use crate::models::{
    BalanceAlgorithm, ConditionExpr, ConditionType, HealthCheck, HealthCheckMode, HostMatchType,
    HostMatcher, LocationMatchType, LocationMatcher, MatchSet, Operator, RetryPolicy, Rule,
    RuleTls, Target, Upstream,
};
use crate::runtime::timeouts::{ConnectionLimitPolicy, TimeoutPolicy};

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn =
            Connection::open(path).with_context(|| format!("failed to open database: {path}"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        conn.execute_batch(SCHEMA)?;
        migrate_schema(&conn)?;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let settings_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
            .unwrap_or(0);
        if settings_count > 0 {
            return Ok(false);
        }

        let upstream_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM upstreams", [], |row| row.get(0))
            .unwrap_or(0);
        let rule_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(upstream_count == 0 && rule_count == 0)
    }

    // ── Config-level operations ──

    pub fn load_config(&self) -> Result<AppConfig> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        load_config(&conn)
    }

    pub fn save_full_config(&self, config: &AppConfig) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        save_full_config(&tx, config)?;
        tx.commit()?;
        Ok(())
    }

    // ── Settings ──

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        get_setting(&conn, key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        set_setting(&conn, key, value)
    }

    pub fn ensure_jwt_secret(&self) -> Result<String> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        if let Some(secret) = get_setting(&conn, "jwt_secret")? {
            return Ok(secret);
        }
        let secret = uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string();
        set_setting(&conn, "jwt_secret", &secret)?;
        Ok(secret)
    }

    // ── Rules CRUD ──

    pub fn list_rules(&self) -> Result<Vec<Rule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let default_listen = load_proxy_listen(&conn)?;
        let mut rules = load_rules(&conn)?;
        for rule in &mut rules {
            AppConfig::normalize_rule_with_default(rule, &default_listen);
        }
        Ok(rules)
    }

    pub fn create_rule(&self, rule: &Rule) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let default_listen = load_proxy_listen(&conn)?;
        let mut rule = rule.clone();
        AppConfig::normalize_rule_with_default(&mut rule, &default_listen);
        let tx = conn.unchecked_transaction()?;
        insert_rule(&tx, &rule)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_rule(&self, rule: &Rule) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let default_listen = load_proxy_listen(&conn)?;
        let mut rule = rule.clone();
        AppConfig::normalize_rule_with_default(&mut rule, &default_listen);
        let tx = conn.unchecked_transaction()?;
        update_rule_row(&tx, &rule)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let changes = conn.execute("DELETE FROM rules WHERE id = ?1", params![id])?;
        Ok(changes > 0)
    }

    // ── Upstreams CRUD ──

    pub fn list_upstreams(&self) -> Result<Vec<Upstream>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        load_upstreams(&conn)
    }

    pub fn create_upstream(&self, upstream: &Upstream) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        insert_upstream(&tx, upstream)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_upstream(&self, upstream: &Upstream) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        delete_upstream_targets(&tx, &upstream.name)?;
        update_upstream_row(&tx, upstream)?;
        insert_targets(&tx, &upstream.name, &upstream.targets)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_upstream(&self, name: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let changes = conn.execute("DELETE FROM upstreams WHERE name = ?1", params![name])?;
        Ok(changes > 0)
    }

    // ── Users ──

    pub fn create_user(&self, username: &str, password_hash: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        conn.execute(
            "INSERT INTO users (username, password) VALUES (?1, ?2)",
            params![username, password_hash],
        )?;
        Ok(())
    }

    pub fn get_user_password_hash(&self, username: &str) -> Result<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let result = conn
            .query_row(
                "SELECT password FROM users WHERE username = ?1",
                params![username],
                |row| row.get::<_, String>(0),
            )
            .ok();
        Ok(result)
    }

    pub fn list_users(&self) -> Result<Vec<(i64, String, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let mut stmt = conn.prepare("SELECT id, username, created_at FROM users ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut users = Vec::new();
        for row in rows {
            users.push(row?);
        }
        Ok(users)
    }

    pub fn update_user_password(&self, username: &str, password_hash: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let changes = conn.execute(
            "UPDATE users SET password = ?1 WHERE username = ?2",
            params![password_hash, username],
        )?;
        Ok(changes > 0)
    }
}

// ── Private helper functions ──

fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let result = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(result)
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

fn load_proxy_listen(conn: &Connection) -> Result<String> {
    Ok(get_setting(conn, "proxy_listen")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0:80".to_string()))
}

fn load_config(conn: &Connection) -> Result<AppConfig> {
    let listen = get_setting(conn, "listen")
        .unwrap_or(None)
        .unwrap_or_else(|| "0.0.0.0:3000".to_string());
    let proxy_listen = get_setting(conn, "proxy_listen")
        .unwrap_or(None)
        .unwrap_or_else(|| "0.0.0.0:80".to_string());
    let fallback_url = get_setting(conn, "fallback_url")
        .unwrap_or(None)
        .unwrap_or_else(|| "404".to_string());
    let connect_timeout = get_setting(conn, "connect_timeout")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let request_timeout = get_setting(conn, "request_timeout")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let pool_max_idle_per_host = get_setting(conn, "pool_max_idle_per_host")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    let pool_idle_timeout = get_setting(conn, "pool_idle_timeout")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
        .unwrap_or(90);
    let tcp_keepalive = get_setting(conn, "tcp_keepalive")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let timeouts = get_setting(conn, "timeouts")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<TimeoutPolicy>(&v).ok())
        .unwrap_or_default();
    let limits = get_setting(conn, "limits")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<ConnectionLimitPolicy>(&v).ok())
        .unwrap_or_default();
    let certificate_dir = get_setting(conn, "certificate_dir")
        .unwrap_or(None)
        .unwrap_or_else(|| "/etc/rustproxy/cert.d".to_string());
    let access_log = get_setting(conn, "access_log")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<AccessLogConfig>(&v).ok())
        .unwrap_or_default();
    let monitoring = get_setting(conn, "monitoring")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<MonitoringConfig>(&v).ok())
        .unwrap_or_default();
    let certificates = get_setting(conn, "certificates")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<Vec<Certificate>>(&v).ok())
        .unwrap_or_default();
    let tls_listeners = get_setting(conn, "tls_listeners")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<Vec<TlsListener>>(&v).ok())
        .unwrap_or_default();

    let match_sets = get_setting(conn, "match_sets")
        .unwrap_or(None)
        .and_then(|v| serde_json::from_str::<Vec<MatchSet>>(&v).ok())
        .unwrap_or_default();
    let rules = load_rules(conn)?;
    let upstreams = load_upstreams(conn)?;

    let mut upstream_map = HashMap::new();
    for u in upstreams {
        upstream_map.insert(u.name.clone(), u);
    }

    let mut config = AppConfig {
        listen,
        proxy_listen,
        timeouts,
        limits,
        connect_timeout,
        request_timeout,
        pool_max_idle_per_host,
        pool_idle_timeout,
        tcp_keepalive,
        certificate_dir,
        access_log,
        monitoring,
        certificates,
        tls_listeners,
        match_sets,
        rules,
        upstreams: upstream_map,
        fallback: Fallback { url: fallback_url },
    };
    config.normalize_timeout_aliases();
    config.normalize_rules();
    Ok(config)
}

fn load_rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, priority, upstream, weight, condition_expr, listen, tls_enabled, tls_certificate, is_fallback, match_set, host_type, host_value, location_type, location_value, request_timeout, header_policy, path_actions, limit_policy FROM rules ORDER BY priority DESC, rowid ASC"
    )?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let priority: i32 = row.get(2)?;
        let upstream: String = row.get(3)?;
        let weight: u32 = row.get::<_, i64>(4)? as u32;
        let expr_json: Option<String> = row.get(5)?;
        let listen: Option<String> = row.get(6)?;
        let tls_enabled: bool = row.get::<_, i64>(7)? != 0;
        let tls_certificate: Option<String> = row.get(8)?;
        let is_fallback: bool = row.get::<_, i64>(9)? != 0;
        let match_set: Option<String> = row.get(10)?;
        let host_type: Option<String> = row.get(11)?;
        let host_value: Option<String> = row.get(12)?;
        let location_type: Option<String> = row.get(13)?;
        let location_value: Option<String> = row.get(14)?;
        let request_timeout: u64 = row.get::<_, i64>(15)? as u64;
        let header_policy_json: Option<String> = row.get(16)?;
        let path_actions_json: Option<String> = row.get(17)?;
        let limit_policy_json: Option<String> = row.get(18)?;
        let conditions =
            expr_json.and_then(|json| serde_json::from_str::<ConditionExpr>(&json).ok());
        Ok(Rule {
            id,
            name,
            priority,
            host: HostMatcher {
                match_type: parse_host_match_type(host_type.as_deref()),
                value: host_value,
            },
            location: LocationMatcher {
                match_type: parse_location_match_type(location_type.as_deref()),
                value: location_value.unwrap_or_else(|| "/".to_string()),
            },
            match_set,
            conditions,
            upstream,
            weight,
            is_fallback,
            listen,
            request_timeout,
            tls: tls_certificate.map(|certificate| RuleTls {
                enabled: tls_enabled,
                certificate,
            }),
            header_policy: json_or_default(header_policy_json.as_deref()),
            path_actions: json_or_default(path_actions_json.as_deref()),
            limit_policy: json_or_default(limit_policy_json.as_deref()),
        })
    })?;

    let mut rules = Vec::new();
    for row in rows {
        rules.push(row?);
    }
    Ok(rules)
}

fn insert_rule(tx: &rusqlite::Transaction, rule: &Rule) -> Result<()> {
    let expr_json = rule
        .conditions
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let tls_enabled = rule.tls.as_ref().is_some_and(|tls| tls.enabled);
    let tls_certificate = rule.tls.as_ref().map(|tls| tls.certificate.as_str());
    let header_policy = serde_json::to_string(&rule.header_policy)?;
    let path_actions = serde_json::to_string(&rule.path_actions)?;
    let limit_policy = serde_json::to_string(&rule.limit_policy)?;
    tx.execute(
        "INSERT INTO rules (id, name, priority, upstream, weight, condition_expr, listen, tls_enabled, tls_certificate, is_fallback, match_set, host_type, host_value, location_type, location_value, request_timeout, header_policy, path_actions, limit_policy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![rule.id, rule.name, rule.priority, rule.upstream, rule.weight, expr_json, rule.listen, tls_enabled, tls_certificate, rule.is_fallback, rule.match_set, host_match_type_str(&rule.host.match_type), rule.host.value, location_match_type_str(&rule.location.match_type), rule.location.value, rule.request_timeout, header_policy, path_actions, limit_policy],
    )?;
    Ok(())
}

fn load_upstreams(conn: &Connection) -> Result<Vec<Upstream>> {
    let mut stmt = conn.prepare("SELECT name FROM upstreams ORDER BY name")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut upstreams = Vec::new();
    for row in rows {
        let name = row?;
        let targets = load_targets(conn, &name)?;
        let (skip_ssl, websocket) = load_upstream_options(conn, &name)?;
        let health_check = load_health_check(conn, &name)?;
        let (balance, retry) = load_upstream_policy(conn, &name)?;
        upstreams.push(Upstream {
            name,
            skip_ssl,
            websocket,
            targets,
            health_check,
            balance,
            retry,
        });
    }
    Ok(upstreams)
}

fn json_or_default<T>(json: Option<&str>) -> T
where
    T: serde::de::DeserializeOwned + Default,
{
    json.and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

fn load_upstream_options(conn: &Connection, upstream_name: &str) -> Result<(bool, bool)> {
    conn.query_row(
        "SELECT skip_ssl, websocket FROM upstreams WHERE name = ?1",
        params![upstream_name],
        |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0)),
    )
    .map_err(Into::into)
}

fn load_health_check(conn: &Connection, upstream_name: &str) -> Result<HealthCheck> {
    conn.query_row(
        "SELECT health_check_enabled, health_check_mode, health_check_path, health_check_expected_status, health_check_interval_seconds, health_check_timeout_seconds, health_check_healthy_threshold, health_check_unhealthy_threshold FROM upstreams WHERE name = ?1",
        params![upstream_name],
        |row| {
            let mode: String = row.get(1)?;
            Ok(HealthCheck {
                enabled: row.get::<_, i64>(0)? != 0,
                mode: if mode == "http" {
                    HealthCheckMode::Http
                } else {
                    HealthCheckMode::Tcp
                },
                path: row.get(2)?,
                expected_status: row.get::<_, i64>(3)? as u16,
                interval_seconds: row.get::<_, i64>(4)? as u64,
                timeout_seconds: row.get::<_, i64>(5)? as u64,
                healthy_threshold: row.get::<_, i64>(6)? as u32,
                unhealthy_threshold: row.get::<_, i64>(7)? as u32,
            })
        },
    )
    .map_err(Into::into)
}

fn load_upstream_policy(
    conn: &Connection,
    upstream_name: &str,
) -> Result<(BalanceAlgorithm, RetryPolicy)> {
    conn.query_row(
        "SELECT balance, retry_policy FROM upstreams WHERE name = ?1",
        params![upstream_name],
        |row| {
            let balance: String = row.get(0)?;
            let retry_json: Option<String> = row.get(1)?;
            Ok((
                parse_balance_algorithm(&balance),
                json_or_default(retry_json.as_deref()),
            ))
        },
    )
    .map_err(Into::into)
}

fn load_targets(conn: &Connection, upstream_name: &str) -> Result<Vec<Target>> {
    let mut stmt = conn
        .prepare("SELECT url, weight FROM targets WHERE upstream_name = ?1 ORDER BY sort_order")?;
    let rows = stmt.query_map(params![upstream_name], |row| {
        Ok(Target {
            url: row.get(0)?,
            weight: row.get::<_, i64>(1)? as u32,
        })
    })?;
    let mut targets = Vec::new();
    for row in rows {
        targets.push(row?);
    }
    Ok(targets)
}

fn save_full_config(tx: &rusqlite::Transaction, config: &AppConfig) -> Result<()> {
    // Clear existing data
    tx.execute("DELETE FROM rules", [])?;
    tx.execute("DELETE FROM targets", [])?;
    tx.execute("DELETE FROM upstreams", [])?;
    tx.execute("DELETE FROM settings", [])?;

    // Settings
    let mut config = config.clone();
    config.normalize_timeout_aliases();
    config.normalize_rules();

    set_setting_tx(tx, "listen", &config.listen)?;
    set_setting_tx(tx, "proxy_listen", &config.proxy_listen)?;
    set_setting_tx(tx, "fallback_url", &config.fallback.url)?;
    set_setting_tx(tx, "timeouts", &serde_json::to_string(&config.timeouts)?)?;
    set_setting_tx(tx, "limits", &serde_json::to_string(&config.limits)?)?;
    set_setting_tx(tx, "connect_timeout", &config.connect_timeout.to_string())?;
    set_setting_tx(tx, "request_timeout", &config.request_timeout.to_string())?;
    set_setting_tx(
        tx,
        "pool_max_idle_per_host",
        &config.pool_max_idle_per_host.to_string(),
    )?;
    set_setting_tx(
        tx,
        "pool_idle_timeout",
        &config.pool_idle_timeout.to_string(),
    )?;
    set_setting_tx(tx, "tcp_keepalive", &config.tcp_keepalive.to_string())?;
    set_setting_tx(tx, "certificate_dir", &config.certificate_dir)?;
    set_setting_tx(
        tx,
        "access_log",
        &serde_json::to_string(&config.access_log)?,
    )?;
    set_setting_tx(
        tx,
        "monitoring",
        &serde_json::to_string(&config.monitoring)?,
    )?;
    set_setting_tx(
        tx,
        "certificates",
        &serde_json::to_string(&config.certificates)?,
    )?;
    set_setting_tx(
        tx,
        "tls_listeners",
        &serde_json::to_string(&config.tls_listeners)?,
    )?;
    set_setting_tx(
        tx,
        "match_sets",
        &serde_json::to_string(&config.match_sets)?,
    )?;

    // Upstreams
    for upstream in config.upstreams.values() {
        insert_upstream(tx, upstream)?;
    }

    // Rules
    for rule in &config.rules {
        insert_rule(tx, rule)?;
    }

    Ok(())
}

fn set_setting_tx(tx: &rusqlite::Transaction, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

fn insert_upstream(tx: &rusqlite::Transaction, upstream: &Upstream) -> Result<()> {
    let retry_policy = serde_json::to_string(&upstream.retry)?;
    tx.execute(
        "INSERT INTO upstreams (name, skip_ssl, websocket, health_check_enabled, health_check_mode, health_check_path, health_check_expected_status, health_check_interval_seconds, health_check_timeout_seconds, health_check_healthy_threshold, health_check_unhealthy_threshold, balance, retry_policy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            upstream.name,
            upstream.skip_ssl,
            upstream.websocket,
            upstream.health_check.enabled,
            health_check_mode_str(&upstream.health_check.mode),
            upstream.health_check.path,
            upstream.health_check.expected_status,
            upstream.health_check.interval_seconds,
            upstream.health_check.timeout_seconds,
            upstream.health_check.healthy_threshold,
            upstream.health_check.unhealthy_threshold,
            balance_algorithm_str(upstream.balance),
            retry_policy,
        ],
    )?;
    insert_targets(tx, &upstream.name, &upstream.targets)?;
    Ok(())
}

fn health_check_mode_str(mode: &HealthCheckMode) -> &'static str {
    match mode {
        HealthCheckMode::Tcp => "tcp",
        HealthCheckMode::Http => "http",
    }
}

fn balance_algorithm_str(balance: BalanceAlgorithm) -> &'static str {
    match balance {
        BalanceAlgorithm::WeightedRoundRobin => "weighted_round_robin",
        BalanceAlgorithm::LeastConnections => "least_connections",
        BalanceAlgorithm::IpHash => "ip_hash",
        BalanceAlgorithm::ConsistentHash => "consistent_hash",
        BalanceAlgorithm::UrlHash => "url_hash",
    }
}

fn parse_balance_algorithm(value: &str) -> BalanceAlgorithm {
    match value {
        "least_connections" => BalanceAlgorithm::LeastConnections,
        "ip_hash" => BalanceAlgorithm::IpHash,
        "consistent_hash" => BalanceAlgorithm::ConsistentHash,
        "url_hash" => BalanceAlgorithm::UrlHash,
        _ => BalanceAlgorithm::WeightedRoundRobin,
    }
}

fn insert_targets(
    tx: &rusqlite::Transaction,
    upstream_name: &str,
    targets: &[Target],
) -> Result<()> {
    for (i, target) in targets.iter().enumerate() {
        tx.execute(
            "INSERT INTO targets (upstream_name, url, weight, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![upstream_name, target.url, target.weight, i as i64],
        )?;
    }
    Ok(())
}

fn update_rule_row(tx: &rusqlite::Transaction, rule: &Rule) -> Result<()> {
    let expr_json = rule
        .conditions
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let tls_enabled = rule.tls.as_ref().is_some_and(|tls| tls.enabled);
    let tls_certificate = rule.tls.as_ref().map(|tls| tls.certificate.as_str());
    let header_policy = serde_json::to_string(&rule.header_policy)?;
    let path_actions = serde_json::to_string(&rule.path_actions)?;
    let limit_policy = serde_json::to_string(&rule.limit_policy)?;
    let changes = tx.execute(
        "UPDATE rules SET name = ?1, priority = ?2, upstream = ?3, weight = ?4, condition_expr = ?5, listen = ?6, tls_enabled = ?7, tls_certificate = ?8, is_fallback = ?9, match_set = ?10, host_type = ?11, host_value = ?12, location_type = ?13, location_value = ?14, request_timeout = ?15, header_policy = ?16, path_actions = ?17, limit_policy = ?18 WHERE id = ?19",
        params![rule.name, rule.priority, rule.upstream, rule.weight, expr_json, rule.listen, tls_enabled, tls_certificate, rule.is_fallback, rule.match_set, host_match_type_str(&rule.host.match_type), rule.host.value, location_match_type_str(&rule.location.match_type), rule.location.value, rule.request_timeout, header_policy, path_actions, limit_policy, rule.id],
    )?;
    if changes == 0 {
        anyhow::bail!("rule '{}' not found", rule.id);
    }
    Ok(())
}

fn parse_host_match_type(value: Option<&str>) -> HostMatchType {
    match value.unwrap_or("any") {
        "exact" => HostMatchType::Exact,
        "wildcard" => HostMatchType::Wildcard,
        _ => HostMatchType::Any,
    }
}

fn host_match_type_str(value: &HostMatchType) -> &'static str {
    match value {
        HostMatchType::Any => "any",
        HostMatchType::Exact => "exact",
        HostMatchType::Wildcard => "wildcard",
    }
}

fn parse_location_match_type(value: Option<&str>) -> LocationMatchType {
    match value.unwrap_or("prefix") {
        "exact" => LocationMatchType::Exact,
        "regex" => LocationMatchType::Regex,
        _ => LocationMatchType::Prefix,
    }
}

fn location_match_type_str(value: &LocationMatchType) -> &'static str {
    match value {
        LocationMatchType::Exact => "exact",
        LocationMatchType::Prefix => "prefix",
        LocationMatchType::Regex => "regex",
    }
}

fn delete_upstream_targets(tx: &rusqlite::Transaction, upstream_name: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM targets WHERE upstream_name = ?1",
        params![upstream_name],
    )?;
    Ok(())
}

fn update_upstream_row(tx: &rusqlite::Transaction, upstream: &Upstream) -> Result<()> {
    let retry_policy = serde_json::to_string(&upstream.retry)?;
    let changes = tx.execute(
        "UPDATE upstreams SET skip_ssl = ?1, websocket = ?2, health_check_enabled = ?3, health_check_mode = ?4, health_check_path = ?5, health_check_expected_status = ?6, health_check_interval_seconds = ?7, health_check_timeout_seconds = ?8, health_check_healthy_threshold = ?9, health_check_unhealthy_threshold = ?10, balance = ?11, retry_policy = ?12, updated_at = datetime('now') WHERE name = ?13",
        params![
            upstream.skip_ssl,
            upstream.websocket,
            upstream.health_check.enabled,
            health_check_mode_str(&upstream.health_check.mode),
            upstream.health_check.path,
            upstream.health_check.expected_status,
            upstream.health_check.interval_seconds,
            upstream.health_check.timeout_seconds,
            upstream.health_check.healthy_threshold,
            upstream.health_check.unhealthy_threshold,
            balance_algorithm_str(upstream.balance),
            retry_policy,
            upstream.name,
        ],
    )?;
    if changes == 0 {
        anyhow::bail!("upstream '{}' not found", upstream.name);
    }
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS upstreams (
    name                             TEXT PRIMARY KEY,
    skip_ssl                         INTEGER NOT NULL DEFAULT 0,
    websocket                        INTEGER NOT NULL DEFAULT 0,
    health_check_enabled             INTEGER NOT NULL DEFAULT 0,
    health_check_mode                TEXT NOT NULL DEFAULT 'tcp',
    health_check_path                TEXT NOT NULL DEFAULT '/health',
    health_check_expected_status     INTEGER NOT NULL DEFAULT 200,
    health_check_interval_seconds    INTEGER NOT NULL DEFAULT 10,
    health_check_timeout_seconds     INTEGER NOT NULL DEFAULT 2,
    health_check_healthy_threshold   INTEGER NOT NULL DEFAULT 2,
    health_check_unhealthy_threshold INTEGER NOT NULL DEFAULT 2,
    balance                          TEXT NOT NULL DEFAULT 'weighted_round_robin',
    retry_policy                     TEXT,
    created_at                       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at                       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS targets (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    upstream_name TEXT NOT NULL REFERENCES upstreams(name) ON DELETE CASCADE,
    url           TEXT NOT NULL,
    weight        INTEGER NOT NULL DEFAULT 100,
    sort_order    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS rules (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 0,
    upstream        TEXT NOT NULL REFERENCES upstreams(name),
    weight          INTEGER NOT NULL DEFAULT 100,
    condition_expr  TEXT,
    listen          TEXT,
    tls_enabled     INTEGER NOT NULL DEFAULT 0,
    tls_certificate TEXT,
    is_fallback     INTEGER NOT NULL DEFAULT 0,
    match_set       TEXT,
    host_type       TEXT NOT NULL DEFAULT 'any',
    host_value      TEXT,
    location_type   TEXT NOT NULL DEFAULT 'prefix',
    location_value  TEXT NOT NULL DEFAULT '/',
    request_timeout INTEGER NOT NULL DEFAULT 0,
    header_policy   TEXT,
    path_actions    TEXT,
    limit_policy    TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    username   TEXT NOT NULL UNIQUE,
    password   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);

CREATE INDEX IF NOT EXISTS idx_targets_upstream ON targets(upstream_name);
CREATE INDEX IF NOT EXISTS idx_rules_priority ON rules(priority DESC);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
"#;

fn migrate_schema(conn: &Connection) -> Result<()> {
    // Check if condition_expr column exists
    let has_condition_expr: bool = conn
        .prepare("SELECT condition_expr FROM rules LIMIT 0")
        .is_ok();

    if !has_condition_expr {
        // Add the new column
        conn.execute_batch("ALTER TABLE rules ADD COLUMN condition_expr TEXT;")?;

        // Migrate data from old conditions table into JSON
        let rule_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM rules")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for rule_id in &rule_ids {
            let expr_json = build_expr_from_conditions(conn, rule_id)?;
            conn.execute(
                "UPDATE rules SET condition_expr = ?1 WHERE id = ?2",
                params![expr_json, rule_id],
            )?;
        }
    }

    // Add listen column if missing
    let has_listen: bool = conn.prepare("SELECT listen FROM rules LIMIT 0").is_ok();
    if !has_listen {
        conn.execute_batch("ALTER TABLE rules ADD COLUMN listen TEXT;")?;
    }
    add_column_if_missing(conn, "rules", "tls_enabled", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "rules", "tls_certificate", "TEXT")?;
    add_column_if_missing(conn, "rules", "is_fallback", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "rules", "match_set", "TEXT")?;
    add_column_if_missing(conn, "rules", "host_type", "TEXT NOT NULL DEFAULT 'any'")?;
    add_column_if_missing(conn, "rules", "host_value", "TEXT")?;
    add_column_if_missing(
        conn,
        "rules",
        "location_type",
        "TEXT NOT NULL DEFAULT 'prefix'",
    )?;
    add_column_if_missing(conn, "rules", "location_value", "TEXT NOT NULL DEFAULT '/'")?;
    add_column_if_missing(
        conn,
        "rules",
        "request_timeout",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "rules", "header_policy", "TEXT")?;
    add_column_if_missing(conn, "rules", "path_actions", "TEXT")?;
    add_column_if_missing(conn, "rules", "limit_policy", "TEXT")?;

    add_column_if_missing(conn, "upstreams", "skip_ssl", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "upstreams", "websocket", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_mode",
        "TEXT NOT NULL DEFAULT 'tcp'",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_path",
        "TEXT NOT NULL DEFAULT '/health'",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_expected_status",
        "INTEGER NOT NULL DEFAULT 200",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_interval_seconds",
        "INTEGER NOT NULL DEFAULT 10",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_timeout_seconds",
        "INTEGER NOT NULL DEFAULT 2",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_healthy_threshold",
        "INTEGER NOT NULL DEFAULT 2",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "health_check_unhealthy_threshold",
        "INTEGER NOT NULL DEFAULT 2",
    )?;
    add_column_if_missing(
        conn,
        "upstreams",
        "balance",
        "TEXT NOT NULL DEFAULT 'weighted_round_robin'",
    )?;
    add_column_if_missing(conn, "upstreams", "retry_policy", "TEXT")?;

    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let exists = conn
        .prepare(&format!("SELECT {column} FROM {table} LIMIT 0"))
        .is_ok();
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))?;
    }
    Ok(())
}

/// Read old conditions table rows for a rule and build a ConditionExpr JSON.
fn build_expr_from_conditions(conn: &Connection, rule_id: &str) -> Result<Option<String>> {
    let mut stmt = match conn.prepare(
        "SELECT condition_type, key, claim_path, operator, value FROM conditions WHERE rule_id = ?1 ORDER BY sort_order"
    ) {
        Ok(s) => s,
        Err(_) => return Ok(None), // conditions table may not exist
    };

    let rows = stmt.query_map(params![rule_id], |row| {
        let type_str: String = row.get(0)?;
        let condition_type = match type_str.as_str() {
            "host" => ConditionType::Host,
            "path" => ConditionType::Path,
            "header" => ConditionType::Header,
            "cookie" => ConditionType::Cookie,
            "jwt" => ConditionType::Jwt,
            _ => ConditionType::Header,
        };
        let op_str: String = row.get(3)?;
        let operator = match op_str.as_str() {
            "exact" => Operator::Exact,
            "prefix" => Operator::Prefix,
            "regex" => Operator::Regex,
            "exists" => Operator::Exists,
            "contains" => Operator::Contains,
            _ => Operator::Exact,
        };
        Ok(ConditionExpr::Leaf {
            condition_type,
            key: row.get(1)?,
            claim_path: row.get(2)?,
            operator,
            value: row.get(4)?,
        })
    })?;

    let mut children: Vec<ConditionExpr> = Vec::new();
    for row in rows {
        children.push(row?);
    }

    if children.is_empty() {
        Ok(None)
    } else if children.len() == 1 {
        Ok(Some(serde_json::to_string(
            &children.into_iter().next().unwrap(),
        )?))
    } else {
        Ok(Some(serde_json::to_string(&ConditionExpr::And {
            children,
        })?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        BalanceAlgorithm, ConditionType, HeaderMutation, HeaderMutationOp, Operator, PathAction,
        RateLimitKey,
    };

    fn make_test_config() -> AppConfig {
        use crate::models::ConditionExpr;
        AppConfig {
            listen: "0.0.0.0:8080".to_string(),
            proxy_listen: "0.0.0.0:80".to_string(),
            timeouts: Default::default(),
            limits: Default::default(),
            match_sets: vec![MatchSet {
                name: "admin-host".to_string(),
                conditions: Some(ConditionExpr::Leaf {
                    condition_type: ConditionType::Host,
                    key: None,
                    claim_path: None,
                    operator: Operator::Exact,
                    value: Some("example.com".to_string()),
                }),
            }],
            rules: vec![Rule {
                id: "rule-1".to_string(),
                name: "Test Rule".to_string(),
                priority: 10,
                host: Default::default(),
                location: Default::default(),
                match_set: None,
                conditions: Some(ConditionExpr::And {
                    children: vec![
                        ConditionExpr::Leaf {
                            condition_type: ConditionType::Header,
                            key: Some("Host".to_string()),
                            claim_path: None,
                            operator: Operator::Exact,
                            value: Some("example.com".to_string()),
                        },
                        ConditionExpr::Leaf {
                            condition_type: ConditionType::Jwt,
                            key: None,
                            claim_path: Some("roles.0".to_string()),
                            operator: Operator::Contains,
                            value: Some("admin".to_string()),
                        },
                    ],
                }),
                upstream: "backend-1".to_string(),
                weight: 100,
                is_fallback: false,
                listen: None,
                request_timeout: 0,
                tls: None,
                header_policy: Default::default(),
                path_actions: Vec::new(),
                limit_policy: Default::default(),
            }],
            upstreams: {
                let mut m = HashMap::new();
                m.insert(
                    "backend-1".to_string(),
                    Upstream {
                        name: "backend-1".to_string(),
                        skip_ssl: false,
                        websocket: false,
                        targets: vec![
                            Target {
                                url: "http://a:8080".to_string(),
                                weight: 70,
                            },
                            Target {
                                url: "http://b:8080".to_string(),
                                weight: 30,
                            },
                        ],
                        health_check: Default::default(),
                        balance: Default::default(),
                        retry: Default::default(),
                    },
                );
                m
            },
            fallback: Fallback {
                url: "http://fallback".to_string(),
            },
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
        }
    }

    #[test]
    fn test_roundtrip() {
        use crate::models::ConditionExpr;
        let db = Database::open_in_memory().unwrap();
        let config = make_test_config();

        db.save_full_config(&config).unwrap();
        let loaded = db.load_config().unwrap();

        assert_eq!(loaded.listen, "0.0.0.0:8080");
        assert_eq!(loaded.fallback.url, "http://fallback");
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].id, "rule-1");
        assert!(loaded.rules[0].conditions.is_some());
        let expr = loaded.rules[0].conditions.as_ref().unwrap();
        match expr {
            ConditionExpr::And { children } => {
                assert_eq!(children.len(), 2);
            }
            _ => panic!("expected And node"),
        }
        assert!(loaded.upstreams.contains_key("backend-1"));
        assert_eq!(loaded.upstreams["backend-1"].targets.len(), 2);
    }

    #[test]
    fn round_trips_proxy_policies() {
        let db = Database::open_in_memory().unwrap();
        let mut config = make_test_config();
        let rule = config.rules.first_mut().unwrap();
        rule.header_policy.request.push(HeaderMutation {
            op: HeaderMutationOp::Set,
            name: "x-forwarded-proto".to_string(),
            value: Some("https".to_string()),
        });
        rule.header_policy.response.push(HeaderMutation {
            op: HeaderMutationOp::Remove,
            name: "server".to_string(),
            value: None,
        });
        rule.path_actions.push(PathAction::StripPrefix {
            prefix: "/api".to_string(),
        });
        rule.limit_policy.rate_per_second = Some(50);
        rule.limit_policy.rate_key = RateLimitKey::Route;
        rule.limit_policy.max_connections = Some(12);
        rule.limit_policy.max_body_bytes = Some(1024 * 1024);
        rule.limit_policy.queue_timeout_ms = Some(250);

        let upstream = config.upstreams.get_mut("backend-1").unwrap();
        upstream.balance = BalanceAlgorithm::LeastConnections;
        upstream.retry.attempts = 2;
        upstream.retry.retry_on_status = vec![502, 503];
        upstream.retry.retry_on_timeout = true;
        upstream.retry.retry_on_connect_error = true;
        let expected_header_policy = rule.header_policy.clone();
        let expected_path_actions = rule.path_actions.clone();
        let expected_limit_policy = rule.limit_policy.clone();

        db.save_full_config(&config).unwrap();
        let loaded = db.load_config().unwrap();
        let loaded_rule = loaded
            .rules
            .iter()
            .find(|rule| rule.id == "rule-1")
            .unwrap();
        assert_eq!(loaded_rule.header_policy, expected_header_policy);
        assert_eq!(loaded_rule.path_actions, expected_path_actions);
        assert_eq!(loaded_rule.limit_policy, expected_limit_policy);

        let loaded_upstream = loaded.upstreams.get("backend-1").unwrap();
        assert_eq!(loaded_upstream.balance, BalanceAlgorithm::LeastConnections);
        assert_eq!(loaded_upstream.retry.attempts, 2);
        assert_eq!(loaded_upstream.retry.retry_on_status, vec![502, 503]);
        assert!(loaded_upstream.retry.retry_on_timeout);
        assert!(loaded_upstream.retry.retry_on_connect_error);
    }

    #[test]
    fn round_trips_timeout_and_limit_settings() {
        let db = Database::open_in_memory().unwrap();
        let mut config = make_test_config();
        config.timeouts = TimeoutPolicy {
            connect_timeout_seconds: 3,
            client_timeout_seconds: 11,
            server_timeout_seconds: 12,
            http_request_timeout_seconds: 13,
            http_keepalive_timeout_seconds: 14,
            tunnel_timeout_seconds: 15,
            queue_timeout_ms: 250,
        };
        config.limits = ConnectionLimitPolicy {
            global_maxconn: Some(1024),
            listener_maxconn: Some(128),
        };

        db.save_full_config(&config).unwrap();
        let loaded = db.load_config().unwrap();

        assert_eq!(loaded.timeouts, config.timeouts);
        assert_eq!(loaded.limits, config.limits);
        assert_eq!(loaded.connect_timeout, 3);
        assert_eq!(loaded.request_timeout, 12);
        assert_eq!(loaded.pool_idle_timeout, 14);
    }

    #[test]
    fn test_is_empty() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.is_empty().unwrap());

        db.set_setting("listen", "0.0.0.0:3000").unwrap();
        assert!(!db.is_empty().unwrap());
    }

    #[test]
    fn test_rule_crud() {
        use crate::models::ConditionExpr;
        let db = Database::open_in_memory().unwrap();
        let config = make_test_config();
        db.save_full_config(&config).unwrap();

        let rules = db.list_rules().unwrap();
        assert_eq!(rules.len(), 1);

        let updated_rule = Rule {
            id: "rule-1".to_string(),
            name: "Updated".to_string(),
            priority: 20,
            host: Default::default(),
            location: Default::default(),
            match_set: None,
            conditions: Some(ConditionExpr::Leaf {
                condition_type: ConditionType::Cookie,
                key: Some("session".to_string()),
                claim_path: None,
                operator: Operator::Exists,
                value: None,
            }),
            upstream: "backend-1".to_string(),
            weight: 50,
            is_fallback: false,
            listen: None,
            request_timeout: 7,
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        };
        db.update_rule(&updated_rule).unwrap();
        let rules = db.list_rules().unwrap();
        assert_eq!(rules[0].name, "Updated");
        assert_eq!(rules[0].priority, 20);
        assert_eq!(rules[0].request_timeout, 7);
        assert!(rules[0].conditions.is_some());

        assert!(db.delete_rule("rule-1").unwrap());
        assert!(db.list_rules().unwrap().is_empty());
    }

    #[test]
    fn test_upstream_crud() {
        let db = Database::open_in_memory().unwrap();
        let config = make_test_config();
        db.save_full_config(&config).unwrap();

        let upstreams = db.list_upstreams().unwrap();
        assert_eq!(upstreams.len(), 1);

        let new_upstream = Upstream {
            name: "backend-2".to_string(),
            skip_ssl: true,
            websocket: true,
            targets: vec![Target {
                url: "http://c:9090".to_string(),
                weight: 100,
            }],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
        };
        db.create_upstream(&new_upstream).unwrap();
        assert_eq!(db.list_upstreams().unwrap().len(), 2);

        let mut updated = new_upstream.clone();
        updated.health_check.enabled = true;
        updated.health_check.mode = HealthCheckMode::Http;
        updated.health_check.path = "/ready".to_string();
        updated.health_check.expected_status = 204;
        db.update_upstream(&updated).unwrap();
        let saved = db
            .list_upstreams()
            .unwrap()
            .into_iter()
            .find(|upstream| upstream.name == "backend-2")
            .unwrap();
        assert!(saved.skip_ssl);
        assert!(saved.websocket);
        assert!(saved.health_check.enabled);
        assert_eq!(saved.health_check.mode, HealthCheckMode::Http);
        assert_eq!(saved.health_check.path, "/ready");
        assert_eq!(saved.health_check.expected_status, 204);

        assert!(db.delete_upstream("backend-2").unwrap());
        assert_eq!(db.list_upstreams().unwrap().len(), 1);
    }

    #[test]
    fn test_user_crud() {
        let db = Database::open_in_memory().unwrap();

        db.create_user("admin", "hash123").unwrap();
        let hash = db.get_user_password_hash("admin").unwrap();
        assert_eq!(hash, Some("hash123".to_string()));

        let users = db.list_users().unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].1, "admin");

        assert!(db.update_user_password("admin", "newhash").unwrap());
        assert_eq!(
            db.get_user_password_hash("admin").unwrap(),
            Some("newhash".to_string())
        );
    }

    #[test]
    fn test_jwt_secret() {
        let db = Database::open_in_memory().unwrap();
        let secret1 = db.ensure_jwt_secret().unwrap();
        let secret2 = db.ensure_jwt_secret().unwrap();
        assert_eq!(secret1, secret2);
        assert!(secret1.len() > 30);
    }
}
