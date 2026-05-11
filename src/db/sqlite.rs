use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::config::yaml::{AppConfig, Fallback};
use crate::models::{
    Condition, ConditionType, Operator, Rule, Target, Upstream,
};

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database: {path}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings WHERE key = 'version'", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(count == 0)
    }

    // ── Config-level operations ──

    pub fn load_config(&self) -> Result<AppConfig> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        load_config(&conn)
    }

    pub fn save_full_config(&self, config: &AppConfig) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        save_full_config(&tx, config)?;
        tx.commit()?;
        Ok(())
    }

    // ── Settings ──

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        get_setting(&conn, key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        set_setting(&conn, key, value)
    }

    pub fn ensure_jwt_secret(&self) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        if let Some(secret) = get_setting(&conn, "jwt_secret")? {
            return Ok(secret);
        }
        let secret = uuid::Uuid::new_v4().to_string()
            + &uuid::Uuid::new_v4().to_string();
        set_setting(&conn, "jwt_secret", &secret)?;
        Ok(secret)
    }

    // ── Rules CRUD ──

    pub fn list_rules(&self) -> Result<Vec<Rule>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        load_rules(&conn)
    }

    pub fn create_rule(&self, rule: &Rule) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        insert_rule(&tx, rule)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_rule(&self, rule: &Rule) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        delete_rule_conditions(&tx, &rule.id)?;
        update_rule_row(&tx, rule)?;
        insert_conditions(&tx, &rule.id, &rule.conditions)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let changes = conn.execute("DELETE FROM rules WHERE id = ?1", params![id])?;
        Ok(changes > 0)
    }

    // ── Upstreams CRUD ──

    pub fn list_upstreams(&self) -> Result<Vec<Upstream>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        load_upstreams(&conn)
    }

    pub fn create_upstream(&self, upstream: &Upstream) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        insert_upstream(&tx, upstream)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_upstream(&self, upstream: &Upstream) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let tx = conn.unchecked_transaction()?;
        delete_upstream_targets(&tx, &upstream.name)?;
        update_upstream_row(&tx, upstream)?;
        insert_targets(&tx, &upstream.name, &upstream.targets)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_upstream(&self, name: &str) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let changes = conn.execute("DELETE FROM upstreams WHERE name = ?1", params![name])?;
        Ok(changes > 0)
    }

    // ── Users ──

    pub fn create_user(&self, username: &str, password_hash: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        conn.execute(
            "INSERT INTO users (username, password) VALUES (?1, ?2)",
            params![username, password_hash],
        )?;
        Ok(())
    }

    pub fn get_user_password_hash(&self, username: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
        let mut stmt = conn.prepare("SELECT id, username, created_at FROM users ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut users = Vec::new();
        for row in rows {
            users.push(row?);
        }
        Ok(users)
    }

    pub fn update_user_password(&self, username: &str, password_hash: &str) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
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

fn load_config(conn: &Connection) -> Result<AppConfig> {
    let version = get_setting(conn, "version").unwrap_or(None).unwrap_or_default();
    let listen = get_setting(conn, "listen").unwrap_or(None).unwrap_or_else(|| "127.0.0.1:3000".to_string());
    let fallback_url = get_setting(conn, "fallback_url").unwrap_or(None).unwrap_or_default();

    let rules = load_rules(conn)?;
    let upstreams = load_upstreams(conn)?;

    let mut upstream_map = HashMap::new();
    for u in upstreams {
        upstream_map.insert(u.name.clone(), u);
    }

    Ok(AppConfig {
        version,
        listen,
        rules,
        upstreams: upstream_map,
        fallback: Fallback { url: fallback_url },
    })
}

fn load_rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, priority, upstream, weight FROM rules ORDER BY priority DESC, id"
    )?;
    let rule_rows = stmt.query_map([], |row| {
        Ok(RowRule {
            id: row.get(0)?,
            name: row.get(1)?,
            priority: row.get(2)?,
            upstream: row.get(3)?,
            weight: row.get::<_, i64>(4)? as u32,
        })
    })?;

    let mut rules = Vec::new();
    for row in rule_rows {
        let r = row?;
        let conditions = load_conditions(conn, &r.id)?;
        rules.push(Rule {
            id: r.id,
            name: r.name,
            priority: r.priority,
            conditions,
            upstream: r.upstream,
            weight: r.weight,
        });
    }
    Ok(rules)
}

struct RowRule {
    id: String,
    name: String,
    priority: i32,
    upstream: String,
    weight: u32,
}

fn load_conditions(conn: &Connection, rule_id: &str) -> Result<Vec<Condition>> {
    let mut stmt = conn.prepare(
        "SELECT condition_type, key, claim_path, operator, value FROM conditions WHERE rule_id = ?1 ORDER BY sort_order"
    )?;
    let rows = stmt.query_map(params![rule_id], |row| {
        let type_str: String = row.get(0)?;
        let condition_type = match type_str.as_str() {
            "header" => ConditionType::Header,
            "cookie" => ConditionType::Cookie,
            "jwt" => ConditionType::Jwt,
            _ => ConditionType::Header,
        };
        let op_str: String = row.get(3)?;
        let operator = match op_str.as_str() {
            "exact" => Operator::Exact,
            "regex" => Operator::Regex,
            "exists" => Operator::Exists,
            "contains" => Operator::Contains,
            _ => Operator::Exact,
        };
        Ok(Condition {
            condition_type,
            key: row.get(1)?,
            claim_path: row.get(2)?,
            operator,
            value: row.get(4)?,
        })
    })?;
    let mut conditions = Vec::new();
    for row in rows {
        conditions.push(row?);
    }
    Ok(conditions)
}

fn load_upstreams(conn: &Connection) -> Result<Vec<Upstream>> {
    let mut stmt = conn.prepare("SELECT name FROM upstreams ORDER BY name")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut upstreams = Vec::new();
    for row in rows {
        let name = row?;
        let targets = load_targets(conn, &name)?;
        upstreams.push(Upstream { name, targets });
    }
    Ok(upstreams)
}

fn load_targets(conn: &Connection, upstream_name: &str) -> Result<Vec<Target>> {
    let mut stmt = conn.prepare(
        "SELECT url, weight FROM targets WHERE upstream_name = ?1 ORDER BY sort_order"
    )?;
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
    tx.execute("DELETE FROM conditions", [])?;
    tx.execute("DELETE FROM rules", [])?;
    tx.execute("DELETE FROM targets", [])?;
    tx.execute("DELETE FROM upstreams", [])?;
    tx.execute("DELETE FROM settings", [])?;

    // Settings
    set_setting_tx(tx, "version", &config.version)?;
    set_setting_tx(tx, "listen", &config.listen)?;
    set_setting_tx(tx, "fallback_url", &config.fallback.url)?;

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
    tx.execute(
        "INSERT INTO upstreams (name) VALUES (?1)",
        params![upstream.name],
    )?;
    insert_targets(tx, &upstream.name, &upstream.targets)?;
    Ok(())
}

fn insert_targets(tx: &rusqlite::Transaction, upstream_name: &str, targets: &[Target]) -> Result<()> {
    for (i, target) in targets.iter().enumerate() {
        tx.execute(
            "INSERT INTO targets (upstream_name, url, weight, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![upstream_name, target.url, target.weight, i as i64],
        )?;
    }
    Ok(())
}

fn insert_rule(tx: &rusqlite::Transaction, rule: &Rule) -> Result<()> {
    tx.execute(
        "INSERT INTO rules (id, name, priority, upstream, weight) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![rule.id, rule.name, rule.priority, rule.upstream, rule.weight],
    )?;
    insert_conditions(tx, &rule.id, &rule.conditions)?;
    Ok(())
}

fn insert_conditions(tx: &rusqlite::Transaction, rule_id: &str, conditions: &[Condition]) -> Result<()> {
    for (i, cond) in conditions.iter().enumerate() {
        let type_str = match cond.condition_type {
            ConditionType::Header => "header",
            ConditionType::Cookie => "cookie",
            ConditionType::Jwt => "jwt",
        };
        let op_str = match cond.operator {
            Operator::Exact => "exact",
            Operator::Regex => "regex",
            Operator::Exists => "exists",
            Operator::Contains => "contains",
        };
        tx.execute(
            "INSERT INTO conditions (rule_id, condition_type, key, claim_path, operator, value, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rule_id, type_str, cond.key, cond.claim_path, op_str, cond.value, i as i64],
        )?;
    }
    Ok(())
}

fn delete_rule_conditions(tx: &rusqlite::Transaction, rule_id: &str) -> Result<()> {
    tx.execute("DELETE FROM conditions WHERE rule_id = ?1", params![rule_id])?;
    Ok(())
}

fn update_rule_row(tx: &rusqlite::Transaction, rule: &Rule) -> Result<()> {
    let changes = tx.execute(
        "UPDATE rules SET name = ?1, priority = ?2, upstream = ?3, weight = ?4 WHERE id = ?5",
        params![rule.name, rule.priority, rule.upstream, rule.weight, rule.id],
    )?;
    if changes == 0 {
        anyhow::bail!("rule '{}' not found", rule.id);
    }
    Ok(())
}

fn delete_upstream_targets(tx: &rusqlite::Transaction, upstream_name: &str) -> Result<()> {
    tx.execute("DELETE FROM targets WHERE upstream_name = ?1", params![upstream_name])?;
    Ok(())
}

fn update_upstream_row(tx: &rusqlite::Transaction, upstream: &Upstream) -> Result<()> {
    let changes = tx.execute(
        "UPDATE upstreams SET updated_at = datetime('now') WHERE name = ?1",
        params![upstream.name],
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
    name       TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS targets (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    upstream_name TEXT NOT NULL REFERENCES upstreams(name) ON DELETE CASCADE,
    url           TEXT NOT NULL,
    weight        INTEGER NOT NULL DEFAULT 100,
    sort_order    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS rules (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 0,
    upstream   TEXT NOT NULL REFERENCES upstreams(name),
    weight     INTEGER NOT NULL DEFAULT 100,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS conditions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id        TEXT NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
    condition_type TEXT NOT NULL CHECK(condition_type IN ('header','cookie','jwt')),
    key            TEXT,
    claim_path     TEXT,
    operator       TEXT NOT NULL CHECK(operator IN ('exact','regex','exists','contains')),
    value          TEXT,
    sort_order     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    username   TEXT NOT NULL UNIQUE,
    password   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);

CREATE INDEX IF NOT EXISTS idx_targets_upstream ON targets(upstream_name);
CREATE INDEX IF NOT EXISTS idx_conditions_rule ON conditions(rule_id);
CREATE INDEX IF NOT EXISTS idx_rules_priority ON rules(priority DESC);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> AppConfig {
        AppConfig {
            version: "1.0".to_string(),
            listen: "0.0.0.0:8080".to_string(),
            rules: vec![
                Rule {
                    id: "rule-1".to_string(),
                    name: "Test Rule".to_string(),
                    priority: 10,
                    conditions: vec![
                        Condition {
                            condition_type: ConditionType::Header,
                            key: Some("Host".to_string()),
                            claim_path: None,
                            operator: Operator::Exact,
                            value: Some("example.com".to_string()),
                        },
                        Condition {
                            condition_type: ConditionType::Jwt,
                            key: None,
                            claim_path: Some("roles.0".to_string()),
                            operator: Operator::Contains,
                            value: Some("admin".to_string()),
                        },
                    ],
                    upstream: "backend-1".to_string(),
                    weight: 100,
                },
            ],
            upstreams: {
                let mut m = HashMap::new();
                m.insert(
                    "backend-1".to_string(),
                    Upstream {
                        name: "backend-1".to_string(),
                        targets: vec![
                            Target { url: "http://a:8080".to_string(), weight: 70 },
                            Target { url: "http://b:8080".to_string(), weight: 30 },
                        ],
                    },
                );
                m
            },
            fallback: Fallback { url: "http://fallback".to_string() },
        }
    }

    #[test]
    fn test_roundtrip() {
        let db = Database::open_in_memory().unwrap();
        let config = make_test_config();

        db.save_full_config(&config).unwrap();
        let loaded = db.load_config().unwrap();

        assert_eq!(loaded.version, "1.0");
        assert_eq!(loaded.listen, "0.0.0.0:8080");
        assert_eq!(loaded.fallback.url, "http://fallback");
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].id, "rule-1");
        assert_eq!(loaded.rules[0].conditions.len(), 2);
        assert_eq!(loaded.rules[0].conditions[0].condition_type, ConditionType::Header);
        assert_eq!(loaded.rules[0].conditions[1].condition_type, ConditionType::Jwt);
        assert!(loaded.upstreams.contains_key("backend-1"));
        assert_eq!(loaded.upstreams["backend-1"].targets.len(), 2);
    }

    #[test]
    fn test_is_empty() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.is_empty().unwrap());

        db.set_setting("version", "1.0").unwrap();
        assert!(!db.is_empty().unwrap());
    }

    #[test]
    fn test_rule_crud() {
        let db = Database::open_in_memory().unwrap();
        let config = make_test_config();
        db.save_full_config(&config).unwrap();

        let rules = db.list_rules().unwrap();
        assert_eq!(rules.len(), 1);

        let updated_rule = Rule {
            id: "rule-1".to_string(),
            name: "Updated".to_string(),
            priority: 20,
            conditions: vec![Condition {
                condition_type: ConditionType::Cookie,
                key: Some("session".to_string()),
                claim_path: None,
                operator: Operator::Exists,
                value: None,
            }],
            upstream: "backend-1".to_string(),
            weight: 50,
        };
        db.update_rule(&updated_rule).unwrap();
        let rules = db.list_rules().unwrap();
        assert_eq!(rules[0].name, "Updated");
        assert_eq!(rules[0].priority, 20);
        assert_eq!(rules[0].conditions.len(), 1);

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
            targets: vec![Target { url: "http://c:9090".to_string(), weight: 100 }],
        };
        db.create_upstream(&new_upstream).unwrap();
        assert_eq!(db.list_upstreams().unwrap().len(), 2);

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
        assert_eq!(db.get_user_password_hash("admin").unwrap(), Some("newhash".to_string()));
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
