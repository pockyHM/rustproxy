use anyhow::{Context, Result};
use http::header::{HeaderName, HeaderValue};
use http::HeaderMap;

use crate::models::{HeaderMutation, HeaderMutationOp, HeaderPolicy};

pub fn apply_request_headers(headers: &mut HeaderMap, policy: &HeaderPolicy) -> Result<()> {
    apply_mutations(headers, &policy.request)
}

pub fn apply_response_headers(headers: &mut HeaderMap, policy: &HeaderPolicy) -> Result<()> {
    apply_mutations(headers, &policy.response)
}

fn apply_mutations(headers: &mut HeaderMap, mutations: &[HeaderMutation]) -> Result<()> {
    for mutation in mutations {
        let name = HeaderName::from_bytes(mutation.name.as_bytes())
            .with_context(|| format!("invalid header name '{}'", mutation.name))?;
        match mutation.op {
            HeaderMutationOp::Set => {
                let value = header_value(mutation)?;
                headers.insert(name, value);
            }
            HeaderMutationOp::Add => {
                let value = header_value(mutation)?;
                headers.append(name, value);
            }
            HeaderMutationOp::Remove => {
                headers.remove(name);
            }
        }
    }
    Ok(())
}

fn header_value(mutation: &HeaderMutation) -> Result<HeaderValue> {
    let value = mutation.value.as_deref().unwrap_or_default();
    HeaderValue::from_str(value)
        .with_context(|| format!("invalid value for header '{}'", mutation.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation(op: HeaderMutationOp, name: &str, value: Option<&str>) -> HeaderMutation {
        HeaderMutation {
            op,
            name: name.to_string(),
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn applies_request_header_set_add_and_remove() {
        let mut headers = HeaderMap::new();
        headers.insert("x-remove", HeaderValue::from_static("old"));
        let policy = HeaderPolicy {
            request: vec![
                mutation(HeaderMutationOp::Set, "x-mode", Some("canary")),
                mutation(HeaderMutationOp::Add, "x-forwarded-for", Some("10.0.0.1")),
                mutation(HeaderMutationOp::Add, "x-forwarded-for", Some("10.0.0.2")),
                mutation(HeaderMutationOp::Remove, "x-remove", None),
            ],
            response: Vec::new(),
        };

        apply_request_headers(&mut headers, &policy).unwrap();

        assert_eq!(headers.get("x-mode").unwrap(), "canary");
        let forwarded: Vec<_> = headers
            .get_all("x-forwarded-for")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(forwarded, vec!["10.0.0.1", "10.0.0.2"]);
        assert!(!headers.contains_key("x-remove"));
    }

    #[test]
    fn applies_response_header_set_add_and_remove() {
        let mut headers = HeaderMap::new();
        headers.insert("server", HeaderValue::from_static("upstream"));
        let policy = HeaderPolicy {
            request: Vec::new(),
            response: vec![
                mutation(HeaderMutationOp::Set, "x-frame-options", Some("DENY")),
                mutation(HeaderMutationOp::Add, "cache-control", Some("no-store")),
                mutation(HeaderMutationOp::Remove, "server", None),
            ],
        };

        apply_response_headers(&mut headers, &policy).unwrap();

        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(headers.get("cache-control").unwrap(), "no-store");
        assert!(!headers.contains_key("server"));
    }
}
