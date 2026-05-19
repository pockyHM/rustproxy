use std::path::PathBuf;

use tokio::{
    fs::OpenOptions,
    io::{self, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

use crate::config::yaml::{AccessLogConfig, AccessLogLevel};

const DEFAULT_BUFFER_SIZE: usize = 8192;

#[derive(Clone)]
pub struct AccessLogger {
    sender: mpsc::Sender<AccessLogEntry>,
    level: AccessLogLevel,
}

#[derive(Debug)]
pub struct AccessLogEntry {
    pub source: String,
    pub method: String,
    pub host: String,
    pub uri: String,
    pub status: u16,
    pub duration_ms: u128,
    pub rule: String,
    pub upstream: String,
    pub target: String,
    pub error: Option<String>,
}

impl AccessLogger {
    pub fn from_config(config: &AccessLogConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        let (sender, receiver) = mpsc::channel(config.buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE));
        let path = config.path.clone().filter(|path| !path.trim().is_empty());
        tokio::spawn(async move {
            if let Err(error) = run_writer(receiver, path).await {
                tracing::warn!(%error, "access log writer stopped");
            }
        });

        Some(Self {
            sender,
            level: config.level,
        })
    }

    pub fn record(&self, entry: AccessLogEntry) {
        if access_log_level(&entry).severity() < self.level.severity() {
            return;
        }

        if self.sender.try_send(entry).is_err() {
            tracing::debug!("access log queue full or closed; dropping log entry");
        }
    }
}

async fn run_writer(
    mut receiver: mpsc::Receiver<AccessLogEntry>,
    path: Option<String>,
) -> io::Result<()> {
    let mut writer: Box<dyn AsyncWrite + Unpin + Send> = match path {
        Some(path) => {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(PathBuf::from(path))
                .await?;
            Box::new(file)
        }
        None => Box::new(io::stdout()),
    };

    while let Some(entry) = receiver.recv().await {
        writer
            .write_all(format_access_log(&entry).as_bytes())
            .await?;
    }
    writer.flush().await
}

fn format_access_log(entry: &AccessLogEntry) -> String {
    let error = entry.error.as_deref().unwrap_or("-");
    let level = access_log_level(entry).as_str();
    format!(
        r#"level="{}" src="{}" method="{}" host="{}" uri="{}" status={} duration_ms={} rule="{}" upstream="{}" target="{}" error="{}""#,
        level,
        sanitize(&entry.source),
        sanitize(&entry.method),
        sanitize(&entry.host),
        sanitize(&entry.uri),
        entry.status,
        entry.duration_ms,
        sanitize(&entry.rule),
        sanitize(&entry.upstream),
        sanitize(&entry.target),
        sanitize(error),
    ) + "\n"
}

fn access_log_level(entry: &AccessLogEntry) -> AccessLogLevel {
    if entry.error.is_some() || entry.status >= 500 {
        AccessLogLevel::Error
    } else if entry.status >= 400 {
        AccessLogLevel::Warn
    } else {
        AccessLogLevel::Info
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{access_log_level, format_access_log, AccessLogEntry};
    use crate::config::yaml::AccessLogLevel;

    #[test]
    fn formats_access_log_line_with_error_reason() {
        let line = format_access_log(&AccessLogEntry {
            source: "127.0.0.1:50888".to_string(),
            method: "GET".to_string(),
            host: "example.com".to_string(),
            uri: "/api?a=1".to_string(),
            status: 502,
            duration_ms: 12,
            rule: "rule-1".to_string(),
            upstream: "api".to_string(),
            target: "http://127.0.0.1:8080".to_string(),
            error: Some("connection refused".to_string()),
        });

        assert!(line.contains(r#"src="127.0.0.1:50888""#));
        assert!(line.contains(r#"level="error""#));
        assert!(line.contains("status=502"));
        assert!(line.contains(r#"upstream="api""#));
        assert!(line.contains(r#"error="connection refused""#));
    }

    #[test]
    fn classifies_access_log_level_from_status_and_error() {
        let mut entry = AccessLogEntry {
            source: "127.0.0.1:50888".to_string(),
            method: "GET".to_string(),
            host: "example.com".to_string(),
            uri: "/api".to_string(),
            status: 200,
            duration_ms: 12,
            rule: "rule-1".to_string(),
            upstream: "api".to_string(),
            target: "http://127.0.0.1:8080".to_string(),
            error: None,
        };
        assert_eq!(access_log_level(&entry), AccessLogLevel::Info);

        entry.status = 404;
        assert_eq!(access_log_level(&entry), AccessLogLevel::Warn);

        entry.status = 502;
        assert_eq!(access_log_level(&entry), AccessLogLevel::Error);

        entry.status = 200;
        entry.error = Some("upstream reset".to_string());
        assert_eq!(access_log_level(&entry), AccessLogLevel::Error);
    }
}
