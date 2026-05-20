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

#[cfg(test)]
mod tests {
    use super::{extract_sticky_key, StickyKeySource, StickyPolicy};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use http::{HeaderValue, Request};
    use serde_json::json;

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
}
