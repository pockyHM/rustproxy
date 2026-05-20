use anyhow::{Context, Result};
use http::{StatusCode, Uri};
use regex::Regex;

use crate::models::PathAction;

pub enum PathDecision {
    Forward(Uri),
    Redirect {
        status: StatusCode,
        location: String,
    },
}

pub fn apply_path_actions(original: &Uri, actions: &[PathAction]) -> Result<PathDecision> {
    let mut path = original.path().to_string();
    let mut query = original.query().map(str::to_string);

    for action in actions {
        match action {
            PathAction::StripPrefix { prefix } => {
                let matches_segment = path == *prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|stripped| stripped.starts_with('/'));
                if matches_segment {
                    let stripped = path.strip_prefix(prefix).unwrap_or_default();
                    path = if stripped.is_empty() {
                        "/".to_string()
                    } else if stripped.starts_with('/') {
                        stripped.to_string()
                    } else {
                        format!("/{stripped}")
                    };
                }
            }
            PathAction::Rewrite {
                pattern,
                replacement,
            } => {
                let regex = Regex::new(pattern)
                    .with_context(|| format!("invalid path regex '{pattern}'"))?;
                let rewritten = regex.replace(&path, replacement.as_str()).to_string();
                if let Some((rewritten_path, rewritten_query)) = rewritten.split_once('?') {
                    path = normalize_path(rewritten_path);
                    query = Some(rewritten_query.to_string());
                } else {
                    path = normalize_path(&rewritten);
                }
            }
            PathAction::Redirect { status, location } => {
                let status = match *status {
                    301 => StatusCode::MOVED_PERMANENTLY,
                    302 => StatusCode::FOUND,
                    _ => anyhow::bail!("redirect status must be 301 or 302"),
                };
                return Ok(PathDecision::Redirect {
                    status,
                    location: location.clone(),
                });
            }
        }
    }

    Ok(PathDecision::Forward(build_relative_uri(
        &path,
        query.as_deref(),
    )?))
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn build_relative_uri(path: &str, query: Option<&str>) -> Result<Uri> {
    let path = normalize_path(path);
    let value = match query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path,
    };
    value
        .parse()
        .with_context(|| format!("invalid rewritten URI '{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_removes_configured_prefix() {
        let uri: Uri = "/api/v1/users?active=true".parse().unwrap();
        let decision = apply_path_actions(
            &uri,
            &[PathAction::StripPrefix {
                prefix: "/api".to_string(),
            }],
        )
        .unwrap();

        match decision {
            PathDecision::Forward(uri) => assert_eq!(uri.to_string(), "/v1/users?active=true"),
            PathDecision::Redirect { .. } => panic!("expected forward"),
        }
    }

    #[test]
    fn strip_prefix_requires_path_segment_boundary() {
        let uri: Uri = "/apix/v1/users".parse().unwrap();
        let decision = apply_path_actions(
            &uri,
            &[PathAction::StripPrefix {
                prefix: "/api".to_string(),
            }],
        )
        .unwrap();

        match decision {
            PathDecision::Forward(uri) => assert_eq!(uri.to_string(), "/apix/v1/users"),
            PathDecision::Redirect { .. } => panic!("expected forward"),
        }
    }

    #[test]
    fn regex_rewrite_can_replace_path_and_query() {
        let uri: Uri = "/v2/users?old=true".parse().unwrap();
        let decision = apply_path_actions(
            &uri,
            &[PathAction::Rewrite {
                pattern: "^/v([0-9]+)/(.*)$".to_string(),
                replacement: "/$2?version=$1".to_string(),
            }],
        )
        .unwrap();

        match decision {
            PathDecision::Forward(uri) => assert_eq!(uri.to_string(), "/users?version=2"),
            PathDecision::Redirect { .. } => panic!("expected forward"),
        }
    }

    #[test]
    fn redirect_returns_status_and_location() {
        let uri: Uri = "/".parse().unwrap();
        let decision = apply_path_actions(
            &uri,
            &[PathAction::Redirect {
                status: 301,
                location: "https://example.com/".to_string(),
            }],
        )
        .unwrap();

        match decision {
            PathDecision::Redirect { status, location } => {
                assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
                assert_eq!(location, "https://example.com/");
            }
            PathDecision::Forward(_) => panic!("expected redirect"),
        }
    }
}
