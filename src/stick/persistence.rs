use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use http::HeaderValue;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{StickyKeySource, StickyPolicy};

#[derive(Debug, Serialize, Deserialize)]
struct StickyCookieClaims {
    key: String,
    exp: usize,
    iat: usize,
}

pub fn new_sticky_key() -> String {
    Uuid::new_v4().to_string()
}

pub fn read_binding_cookie(
    cookie_header: Option<&str>,
    policy: &StickyPolicy,
    secret: &str,
) -> Option<String> {
    if !policy.enabled || policy.cookie.is_none() {
        return None;
    }
    let cookie_name = cookie_name(policy)?;
    let token = cookie_header
        .unwrap_or_default()
        .split(';')
        .find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name == cookie_name).then_some(value)
        })?;
    decode::<StickyCookieClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims.key)
    .filter(|key| !key.is_empty())
}

pub fn issue_binding_cookie(policy: &StickyPolicy, key: &str, secret: &str) -> Result<HeaderValue> {
    let cookie = policy
        .cookie
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("sticky cookie policy is not configured"))?;
    let now = now_seconds()?;
    let claims = StickyCookieClaims {
        key: key.to_string(),
        iat: now,
        exp: now.saturating_add(policy.ttl_seconds as usize),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    let mut value = format!(
        "{}={}; Max-Age={}; Path={}",
        cookie.name,
        token,
        policy.ttl_seconds,
        cookie.path.as_deref().unwrap_or("/")
    );
    if cookie.secure {
        value.push_str("; Secure");
    }
    if cookie.http_only {
        value.push_str("; HttpOnly");
    }
    if let Some(same_site) = cookie
        .same_site
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        value.push_str("; SameSite=");
        value.push_str(same_site);
    }
    HeaderValue::from_str(&value).map_err(Into::into)
}

pub fn cookie_name(policy: &StickyPolicy) -> Option<&str> {
    policy
        .cookie
        .as_ref()
        .map(|cookie| cookie.name.as_str())
        .or_else(|| match &policy.source {
            StickyKeySource::Cookie { name } => Some(name.as_str()),
            _ => None,
        })
}

fn now_seconds() -> Result<usize> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as usize)
}

#[cfg(test)]
mod tests {
    use super::{issue_binding_cookie, read_binding_cookie};
    use crate::stick::{StickyCookiePolicy, StickyKeySource, StickyPolicy};

    fn policy() -> StickyPolicy {
        StickyPolicy {
            enabled: true,
            source: StickyKeySource::Cookie {
                name: "rp_stick".to_string(),
            },
            ttl_seconds: 300,
            cookie: Some(StickyCookiePolicy {
                name: "rp_stick".to_string(),
                path: Some("/app".to_string()),
                secure: true,
                http_only: true,
                same_site: Some("Lax".to_string()),
            }),
        }
    }

    #[test]
    fn issued_cookie_contains_configured_attributes() {
        let value = issue_binding_cookie(&policy(), "sticky-key-1", "secret").unwrap();
        let cookie = value.to_str().unwrap();

        assert!(cookie.starts_with("rp_stick="));
        assert!(cookie.contains("; Max-Age=300"));
        assert!(cookie.contains("; Path=/app"));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("; HttpOnly"));
        assert!(cookie.contains("; SameSite=Lax"));
        assert!(!cookie.contains("http://"));
    }

    #[test]
    fn issued_cookie_round_trips_binding_key() {
        let value = issue_binding_cookie(&policy(), "sticky-key-1", "secret").unwrap();
        let cookie = value.to_str().unwrap();

        assert_eq!(
            read_binding_cookie(Some(cookie), &policy(), "secret"),
            Some("sticky-key-1".to_string())
        );
        assert_eq!(read_binding_cookie(Some(cookie), &policy(), "wrong"), None);
    }
}
