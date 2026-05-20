use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::http::Uri;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Semaphore};

use crate::config::yaml::{AppConfig, TcpListenerConfig, TcpListenerMode};
use crate::observability::metrics::ProxyMetrics;
use crate::proxy::balancer::{BalanceContext, Balancer};
use crate::runtime::drain::DrainController;
use crate::runtime::timeouts::ResolvedTimeoutPolicy;

#[derive(Clone)]
pub struct TcpRuntimeSnapshot {
    pub config: Arc<AppConfig>,
    pub balancer: Arc<Balancer>,
}

pub trait TcpRuntime: Send + Sync {
    fn snapshot(&self) -> TcpRuntimeSnapshot;
}

pub async fn run_tcp_listener(
    listener: TcpListener,
    config: TcpListenerConfig,
    runtime: Arc<dyn TcpRuntime>,
    metrics: Arc<ProxyMetrics>,
    drain: DrainController,
    mut shutdown: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let maxconn = config
        .maxconn
        .map(|maxconn| Arc::new(Semaphore::new(maxconn as usize)));

    loop {
        let (stream, remote_addr) = tokio::select! {
            result = listener.accept() => result?,
            _ = &mut shutdown => break,
        };
        let Some(drain_lease) = drain.try_acquire() else {
            break;
        };
        let maxconn_permit = match maxconn.as_ref() {
            Some(semaphore) => match semaphore.clone().try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => continue,
            },
            None => None,
        };
        let config = config.clone();
        let runtime = Arc::clone(&runtime);
        let metrics = Arc::clone(&metrics);
        let remote_ip = remote_addr.ip().to_string();
        tokio::spawn(async move {
            let _drain_lease = drain_lease;
            let _maxconn_permit = maxconn_permit;
            if let Err(error) =
                handle_tcp_connection(stream, config, runtime, metrics, remote_ip).await
            {
                tracing::warn!(%error, "TCP proxy connection failed");
            }
        });
    }

    Ok(())
}

async fn handle_tcp_connection(
    mut downstream: TcpStream,
    config: TcpListenerConfig,
    runtime: Arc<dyn TcpRuntime>,
    metrics: Arc<ProxyMetrics>,
    remote_ip: String,
) -> anyhow::Result<()> {
    let upstream = match config.mode {
        TcpListenerMode::Tcp => config.upstream.as_deref(),
        // Task 7 adds ClientHello SNI routing. Until then a default upstream can still work.
        TcpListenerMode::TlsPassthrough => config.upstream.as_deref(),
    }
    .filter(|upstream| !upstream.trim().is_empty())
    .context("TCP listener has no upstream")?;

    let runtime = runtime.snapshot();
    let selected = runtime
        .balancer
        .select(
            upstream,
            BalanceContext {
                client_ip: Some(remote_ip.as_str()),
                path: "",
            },
        )
        .with_context(|| format!("no selectable target for TCP upstream {upstream}"))?;
    let target = selected.url.clone();
    let _target_lease = selected.active_connection;
    let target_addr = target_socket_addr(&target)
        .with_context(|| format!("invalid TCP target address {target}"))?;
    let connect_timeout = resolved_connect_timeout(&runtime.config, upstream, &target);
    let connect = TcpStream::connect(&target_addr);
    let mut upstream_stream = timeout_optional(connect_timeout, connect)
        .await
        .context("TCP upstream connect timed out")?
        .with_context(|| format!("failed to connect TCP upstream {target_addr}"))?;

    let started = Instant::now();
    metrics
        .tcp_connections_total
        .with_label_values(&[config.listen.as_str(), upstream, target.as_str()])
        .inc();
    let copy_result = tokio::io::copy_bidirectional(&mut downstream, &mut upstream_stream).await;
    metrics
        .tcp_connection_duration
        .with_label_values(&[config.listen.as_str(), upstream, target.as_str()])
        .observe(started.elapsed().as_secs_f64());
    let (client_to_upstream, upstream_to_client) = copy_result?;
    metrics
        .tcp_bytes_total
        .with_label_values(&[
            config.listen.as_str(),
            upstream,
            target.as_str(),
            "client_to_upstream",
        ])
        .inc_by(client_to_upstream as f64);
    metrics
        .tcp_bytes_total
        .with_label_values(&[
            config.listen.as_str(),
            upstream,
            target.as_str(),
            "upstream_to_client",
        ])
        .inc_by(upstream_to_client as f64);
    Ok(())
}

fn resolved_connect_timeout(config: &AppConfig, upstream_name: &str, target_url: &str) -> Duration {
    let upstream = config.upstreams.get(upstream_name);
    let target = upstream.and_then(|upstream| {
        upstream
            .targets
            .iter()
            .find(|target| target.url == target_url)
    });
    ResolvedTimeoutPolicy::resolve(
        &config.timeouts,
        None,
        upstream.map(|upstream| &upstream.timeouts),
        target.map(|target| &target.timeouts),
    )
    .connect_timeout
}

async fn timeout_optional<F, T>(
    duration: Duration,
    future: F,
) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    if duration.is_zero() {
        Ok(future.await)
    } else {
        tokio::time::timeout(duration, future).await
    }
}

fn target_socket_addr(target_url: &str) -> Option<String> {
    if target_url.parse::<std::net::SocketAddr>().is_ok() {
        return Some(target_url.to_string());
    }
    let uri: Uri = target_url.parse().ok()?;
    let host = uri.host()?;
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("http") => Some(80),
        Some("https") => Some(443),
        _ => None,
    })?;
    Some(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::target_socket_addr;

    #[test]
    fn extracts_tcp_target_authority() {
        assert_eq!(
            target_socket_addr("tcp://127.0.0.1:6379"),
            Some("127.0.0.1:6379".to_string())
        );
        assert_eq!(
            target_socket_addr("127.0.0.1:6379"),
            Some("127.0.0.1:6379".to_string())
        );
    }
}
