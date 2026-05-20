use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http::Request;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StickyKeySource {
    Ip,
    Header { name: String },
    Cookie { name: String },
    JwtClaim { claim_path: String },
}

impl Default for StickyKeySource {
    fn default() -> Self {
        Self::Ip
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StickyCookiePolicy {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub same_site: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StickyPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub source: StickyKeySource,
    #[serde(default = "default_sticky_ttl_seconds")]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub cookie: Option<StickyCookiePolicy>,
}

impl Default for StickyPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            source: StickyKeySource::Ip,
            ttl_seconds: default_sticky_ttl_seconds(),
            cookie: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StickTable {
    entries: Arc<Mutex<HashMap<StickKey, StickEntry>>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct StickKey {
    upstream: String,
    key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickEntry {
    pub target: String,
    pub expires_at: Instant,
    pub request_count: u64,
    pub error_count: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickSnapshotEntry {
    pub upstream: String,
    pub key: String,
    pub target: String,
    pub expires_at: Instant,
    pub request_count: u64,
    pub error_count: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

fn default_sticky_ttl_seconds() -> u64 {
    3600
}

pub fn extract_sticky_key<B>(
    policy: &StickyPolicy,
    request: &Request<B>,
    client_ip: Option<&str>,
) -> Option<String> {
    if !policy.enabled {
        return None;
    }

    non_empty(match &policy.source {
        StickyKeySource::Ip => client_ip.map(str::to_string),
        StickyKeySource::Header { name } => request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        StickyKeySource::Cookie { name } => request
            .headers()
            .get(http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|cookie| cookie_value(cookie, name))
            .map(str::to_string),
        StickyKeySource::JwtClaim { claim_path } => request
            .headers()
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(decode_jwt_payload)
            .and_then(|payload| {
                let path = claim_path.split('.').filter(|part| !part.is_empty());
                navigate_path(&payload, path).and_then(value_to_string)
            }),
    })
}

impl StickTable {
    pub fn lookup(&self, upstream: &str, key: &str, now: Instant) -> Option<String> {
        let mut entries = self.entries.lock().expect("stick table lock poisoned");
        prune_expired(&mut entries, now);
        let entry = entries.get_mut(&StickKey::new(upstream, key))?;
        entry.request_count = entry.request_count.saturating_add(1);
        Some(entry.target.clone())
    }

    pub fn bind(&self, upstream: &str, key: &str, target: &str, expires_at: Instant, now: Instant) {
        let mut entries = self.entries.lock().expect("stick table lock poisoned");
        prune_expired(&mut entries, now);
        entries
            .entry(StickKey::new(upstream, key))
            .and_modify(|entry| {
                if entry.target != target {
                    entry.request_count = 0;
                    entry.error_count = 0;
                    entry.bytes_in = 0;
                    entry.bytes_out = 0;
                }
                entry.target = target.to_string();
                entry.expires_at = expires_at;
            })
            .or_insert_with(|| StickEntry {
                target: target.to_string(),
                expires_at,
                request_count: 0,
                error_count: 0,
                bytes_in: 0,
                bytes_out: 0,
            });
    }

    pub fn snapshot(&self, now: Instant) -> Vec<StickSnapshotEntry> {
        let mut entries = self.entries.lock().expect("stick table lock poisoned");
        prune_expired(&mut entries, now);
        entries
            .iter()
            .map(|(key, entry)| StickSnapshotEntry {
                upstream: key.upstream.clone(),
                key: key.key.clone(),
                target: entry.target.clone(),
                expires_at: entry.expires_at,
                request_count: entry.request_count,
                error_count: entry.error_count,
                bytes_in: entry.bytes_in,
                bytes_out: entry.bytes_out,
            })
            .collect()
    }
}

impl StickKey {
    fn new(upstream: &str, key: &str) -> Self {
        Self {
            upstream: upstream.to_string(),
            key: key.to_string(),
        }
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.splitn(3, '.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn navigate_path<'a>(
    value: &'a Value,
    path: impl IntoIterator<Item = &'a str>,
) -> Option<&'a Value> {
    let mut current = value;
    for part in path {
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn prune_expired(entries: &mut HashMap<StickKey, StickEntry>, now: Instant) {
    entries.retain(|_, entry| entry.expires_at > now);
}

#[cfg(test)]
mod tests {
    use super::{extract_sticky_key, StickTable, StickyKeySource, StickyPolicy};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use http::{HeaderValue, Request};
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn test_request() -> Request<()> {
        Request::builder().uri("/").body(()).unwrap()
    }

    fn unsigned_jwt(claims: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap().as_bytes());
        format!("{header}.{payload}.")
    }

    #[test]
    fn extracts_sticky_keys() {
        let request = test_request();
        let policy = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Ip,
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(
            extract_sticky_key(&policy, &request, Some("203.0.113.7")),
            Some("203.0.113.7".to_string())
        );

        let mut request = test_request();
        request
            .headers_mut()
            .insert("x-session", HeaderValue::from_static("abc-123"));
        let policy = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Header {
                name: "x-session".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(
            extract_sticky_key(&policy, &request, Some("203.0.113.7")),
            Some("abc-123".to_string())
        );

        let mut request = test_request();
        request.headers_mut().insert(
            "cookie",
            HeaderValue::from_static("theme=dark; session=xyz789"),
        );
        let policy = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Cookie {
                name: "session".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(
            extract_sticky_key(&policy, &request, Some("203.0.113.7")),
            Some("xyz789".to_string())
        );

        let token = unsigned_jwt(json!({
            "sub": "user-1",
            "tenant": { "id": 42 }
        }));
        let mut request = test_request();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let policy = StickyPolicy {
            enabled: true,
            source: StickyKeySource::JwtClaim {
                claim_path: "tenant.id".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(
            extract_sticky_key(&policy, &request, Some("203.0.113.7")),
            Some("42".to_string())
        );
    }

    #[test]
    fn missing_sticky_keys_return_none() {
        let request = test_request();
        let disabled = StickyPolicy::default();
        assert_eq!(
            extract_sticky_key(&disabled, &request, Some("203.0.113.7")),
            None
        );

        let no_ip = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Ip,
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(extract_sticky_key(&no_ip, &request, None), None);

        let missing_header = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Header {
                name: "x-session".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(extract_sticky_key(&missing_header, &request, None), None);

        let missing_cookie = StickyPolicy {
            enabled: true,
            source: StickyKeySource::Cookie {
                name: "session".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(extract_sticky_key(&missing_cookie, &request, None), None);

        let token = unsigned_jwt(json!({ "tenant": {} }));
        let mut request = test_request();
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let missing_claim = StickyPolicy {
            enabled: true,
            source: StickyKeySource::JwtClaim {
                claim_path: "tenant.id".to_string(),
            },
            ttl_seconds: 60,
            cookie: None,
        };
        assert_eq!(extract_sticky_key(&missing_claim, &request, None), None);
    }

    #[test]
    fn same_key_reuses_bound_target() {
        let table = StickTable::default();
        let now = Instant::now();
        table.bind(
            "backend",
            "user-1",
            "http://a",
            now + Duration::from_secs(60),
            now,
        );

        assert_eq!(
            table.lookup("backend", "user-1", now),
            Some("http://a".to_string())
        );
        assert_eq!(
            table.lookup("backend", "user-1", now + Duration::from_secs(30)),
            Some("http://a".to_string())
        );
        assert_eq!(
            table
                .snapshot(now + Duration::from_secs(30))
                .first()
                .map(|entry| entry.request_count),
            Some(2)
        );
    }

    #[test]
    fn expired_bindings_are_pruned() {
        let table = StickTable::default();
        let now = Instant::now();
        table.bind(
            "backend",
            "user-1",
            "http://a",
            now + Duration::from_secs(1),
            now,
        );

        assert_eq!(
            table.lookup("backend", "user-1", now + Duration::from_secs(2)),
            None
        );
        assert!(table.snapshot(now + Duration::from_secs(2)).is_empty());
    }
}
