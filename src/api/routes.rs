use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    future::Future,
    hash::{Hash, Hasher},
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use arc_swap::ArcSwap;
use axum::{
    body::Body,
    extract::{ConnectInfo, Extension, State},
    http::{Request, Response, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{any, get, post, put},
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use rustls::pki_types::pem::PemObject;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tower_http::trace::TraceLayer;

use crate::models::{
    ConditionExpr, ConditionType, HostMatchType, LimitPolicy, LocationMatchType, Rule,
};
use crate::{
    auth::middleware::{self as auth_mw},
    config::yaml::{AppConfig, TcpListenerConfig},
    db::Database,
    observability::{access_log::AccessLogger, metrics::ProxyMetrics},
    proxy::balancer::{BalanceContext, Balancer},
    proxy::health::{run_health_checks, ConfigSnapshot, HealthRegistry},
    proxy::limits::{LimitContext, LimitState},
    proxy::matcher::Matcher,
    proxy::request_for_matching,
    proxy::ProxyClients,
    proxy::{
        handle_proxy_with_target, ProxyAccessLogContext, ProxyMetricLabels, ProxyRequestContext,
    },
    runtime::api as runtime_api,
    runtime::drain::DrainController,
    runtime::state::RuntimeState,
    runtime::timeouts::ResolvedTimeoutPolicy,
    tcp::{run_tcp_listener, TcpRuntime, TcpRuntimeSnapshot},
};

use super::handlers;

const LISTENER_DRAIN_HARD_TIMEOUT: Duration = Duration::from_secs(30);

/// Pre-built proxy runtime shared across all requests.
/// Replaced atomically when config changes — includes clients for hot-reload.
#[derive(Clone)]
struct ProxyRuntime {
    matcher: Arc<Matcher>,
    balancer: Arc<Balancer>,
    config: Arc<AppConfig>,
    clients: Arc<ProxyClients>,
    access_logger: Option<Arc<AccessLogger>>,
    limits: Arc<LimitState>,
}

struct SharedTcpRuntime {
    proxy_runtime: Arc<ArcSwap<ProxyRuntime>>,
}

impl TcpRuntime for SharedTcpRuntime {
    fn snapshot(&self) -> TcpRuntimeSnapshot {
        let runtime = self.proxy_runtime.load_full();
        TcpRuntimeSnapshot {
            config: runtime.config.clone(),
            balancer: runtime.balancer.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListenerProtocol {
    Http,
    Https,
    Tcp,
}

#[derive(Clone)]
struct ListenerSpec {
    listen: String,
    protocol: ListenerProtocol,
    signature: String,
    acceptor: Option<TlsAcceptor>,
    tcp_listener: Option<TcpListenerConfig>,
}

struct ListenerHandle {
    spec: ListenerSpec,
    shutdown: oneshot::Sender<()>,
    join: JoinHandle<()>,
    drain: DrainController,
}

#[derive(Default)]
struct ListenerManager {
    handles: RwLock<HashMap<String, ListenerHandle>>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: Arc<Database>,
    pub jwt_secret: Arc<String>,
    pub metrics: Arc<ProxyMetrics>,
    pub health: HealthRegistry,
    health_config: ConfigSnapshot,
    /// Hot-path proxy runtime — lock-free reads via ArcSwap, atomically swapped on config change.
    proxy_runtime: Arc<ArcSwap<ProxyRuntime>>,
    runtime_state: RuntimeState,
    listener_manager: Arc<ListenerManager>,
    listener_lifecycle: Arc<Mutex<()>>,
    shutting_down: Arc<AtomicBool>,
}

impl AppState {
    pub(crate) fn runtime_state(&self) -> RuntimeState {
        self.runtime_state.clone()
    }

    #[cfg(test)]
    pub(crate) const TEST_JWT_SECRET: &'static str = "test-secret";

    #[cfg(test)]
    pub(crate) fn runtime_state_for_test(&self) -> RuntimeState {
        self.runtime_state.clone()
    }

    #[cfg(test)]
    pub(crate) fn for_test(config: AppConfig) -> Self {
        crate::install_rustls_crypto_provider();
        let health = HealthRegistry::new();
        let health_config = ConfigSnapshot::new();
        health_config.update(&config.upstreams);
        let runtime_state = RuntimeState::default();
        let jwt_secret = Self::TEST_JWT_SECRET.to_string();
        let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
            matcher: Arc::new(Matcher::new_verified_with_match_sets(
                config.rules.clone(),
                config.match_sets.clone(),
                jwt_secret.clone(),
            )),
            balancer: Arc::new(Balancer::new_with_runtime(
                config.upstreams.clone(),
                Some(health.clone()),
                runtime_state.clone(),
            )),
            config: Arc::new(config.clone()),
            clients: Arc::new(ProxyClients::new(None, 32, None, None)),
            access_logger: None,
            limits: Arc::new(LimitState::default()),
        }));

        Self {
            config: Arc::new(RwLock::new(config)),
            db: Arc::new(Database::open_in_memory().unwrap()),
            jwt_secret: Arc::new(jwt_secret),
            metrics: Arc::new(ProxyMetrics::new().unwrap()),
            health,
            health_config,
            proxy_runtime,
            runtime_state,
            listener_manager: Arc::new(ListenerManager::default()),
            listener_lifecycle: Arc::new(Mutex::new(())),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn rebuild_proxy_runtime(&self, old: &AppConfig, new: &AppConfig) {
        let clients_changed = old.connect_timeout != new.connect_timeout
            || old.pool_max_idle_per_host != new.pool_max_idle_per_host
            || old.pool_idle_timeout != new.pool_idle_timeout
            || old.tcp_keepalive != new.tcp_keepalive;
        let access_logger = if old.access_log != new.access_log {
            AccessLogger::from_config(&new.access_log).map(Arc::new)
        } else {
            self.proxy_runtime.load().access_logger.clone()
        };

        let clients = if clients_changed {
            Arc::new(ProxyClients::new(
                if new.connect_timeout > 0 {
                    Some(std::time::Duration::from_secs(new.connect_timeout))
                } else {
                    None
                },
                new.pool_max_idle_per_host,
                if new.pool_idle_timeout > 0 {
                    Some(std::time::Duration::from_secs(new.pool_idle_timeout))
                } else {
                    None
                },
                if new.tcp_keepalive > 0 {
                    Some(std::time::Duration::from_secs(new.tcp_keepalive))
                } else {
                    None
                },
            ))
        } else {
            // Reuse existing connection pool
            self.proxy_runtime.load().clients.clone()
        };

        let runtime = ProxyRuntime {
            matcher: Arc::new(Matcher::new_verified_with_match_sets(
                new.rules.clone(),
                new.match_sets.clone(),
                self.jwt_secret.to_string(),
            )),
            balancer: Arc::new(Balancer::new_with_runtime(
                new.upstreams.clone(),
                Some(self.health.clone()),
                self.runtime_state.clone(),
            )),
            config: Arc::new(new.clone()),
            clients,
            access_logger,
            limits: Arc::new(LimitState::default()),
        };

        self.health_config.update(&new.upstreams);
        self.proxy_runtime.store(Arc::new(runtime));
    }

    pub(crate) async fn sync_proxy_listeners(&self, config: &AppConfig) -> anyhow::Result<()> {
        let _lifecycle = self.listener_lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Ok(());
        }
        self.listener_manager
            .sync(self.clone(), config)
            .await
            .context("failed to sync proxy listeners")
    }

    pub(crate) async fn shutdown_proxy_listeners(&self) {
        let _lifecycle = self.listener_lifecycle.lock().await;
        self.shutting_down.store(true, Ordering::Release);
        self.listener_manager.shutdown_all().await;
    }

    fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }
}

impl ListenerManager {
    async fn sync(&self, state: AppState, config: &AppConfig) -> anyhow::Result<()> {
        let desired = proxy_listener_specs(config)?;
        let mut handles = self.handles.write().await;

        let mut additions = Vec::new();
        let mut removals = Vec::new();
        let mut replacements = Vec::new();

        for (listen, desired_spec) in &desired {
            match handles.get(listen) {
                Some(current)
                    if current.spec.protocol == desired_spec.protocol
                        && current.spec.signature == desired_spec.signature => {}
                Some(_) => replacements.push((listen.clone(), desired_spec.clone())),
                None => additions.push(desired_spec.clone()),
            }
        }

        for listen in handles.keys() {
            if !desired.contains_key(listen) {
                removals.push(listen.clone());
            }
        }

        let mut started = Vec::new();
        for spec in additions {
            if state.is_shutting_down() {
                for (_, handle) in started {
                    stop_listener(handle).await;
                }
                return Ok(());
            }
            match start_listener(state.clone(), spec.clone()).await {
                Ok(handle) => {
                    if state.is_shutting_down() {
                        stop_listener(handle).await;
                        for (_, handle) in started {
                            stop_listener(handle).await;
                        }
                        return Ok(());
                    }
                    tracing::info!(listen = %spec.listen, protocol = ?spec.protocol, "proxy listener hot-added");
                    started.push((spec.listen.clone(), handle));
                }
                Err(error) => {
                    for (_, handle) in started {
                        stop_listener(handle).await;
                    }
                    return Err(error);
                }
            }
        }
        let added_keys: Vec<_> = started.iter().map(|(listen, _)| listen.clone()).collect();
        for (listen, handle) in started {
            handles.insert(listen, handle);
        }

        let mut replaced = Vec::new();
        for (listen, desired_spec) in replacements {
            if state.is_shutting_down() {
                return Ok(());
            }
            let Some(old_handle) = handles.remove(&listen) else {
                continue;
            };
            let old_spec = old_handle.spec.clone();
            tracing::info!(
                listen = %listen,
                from = ?old_spec.protocol,
                to = ?desired_spec.protocol,
                "proxy listener restarting for config change"
            );
            stop_listener(old_handle).await;

            if state.is_shutting_down() {
                return Ok(());
            }
            match start_listener(state.clone(), desired_spec.clone()).await {
                Ok(new_handle) => {
                    if state.is_shutting_down() {
                        stop_listener(new_handle).await;
                        return Ok(());
                    }
                    handles.insert(listen, new_handle);
                    replaced.push((old_spec.listen.clone(), old_spec));
                }
                Err(error) => {
                    tracing::error!(listen = %old_spec.listen, %error, "failed to start replacement listener; attempting rollback");
                    for added_key in &added_keys {
                        if let Some(added) = handles.remove(added_key) {
                            stop_listener(added).await;
                        }
                    }
                    for (replaced_listen, replaced_old_spec) in replaced.into_iter().rev() {
                        if let Some(replaced_new) = handles.remove(&replaced_listen) {
                            stop_listener(replaced_new).await;
                        }
                        if state.is_shutting_down() {
                            continue;
                        }
                        match start_listener(state.clone(), replaced_old_spec.clone()).await {
                            Ok(restored) => {
                                if state.is_shutting_down() {
                                    stop_listener(restored).await;
                                } else {
                                    handles.insert(replaced_old_spec.listen.clone(), restored);
                                }
                            }
                            Err(restore_error) => {
                                tracing::error!(
                                    listen = %replaced_old_spec.listen,
                                    %restore_error,
                                    "failed to restore previous listener after replacement rollback"
                                );
                            }
                        }
                    }
                    if !state.is_shutting_down() {
                        match start_listener(state.clone(), old_spec.clone()).await {
                            Ok(restored) => {
                                if state.is_shutting_down() {
                                    stop_listener(restored).await;
                                } else {
                                    handles.insert(old_spec.listen.clone(), restored);
                                }
                            }
                            Err(restore_error) => {
                                tracing::error!(
                                    listen = %old_spec.listen,
                                    %restore_error,
                                    "failed to restore previous listener after replacement failure"
                                );
                            }
                        }
                    }
                    return Err(error);
                }
            }
        }

        for listen in removals {
            if let Some(handle) = handles.remove(&listen) {
                tracing::info!(listen = %listen, protocol = ?handle.spec.protocol, "proxy listener removed");
                stop_listener(handle).await;
            }
        }

        Ok(())
    }

    async fn shutdown_all(&self) {
        let handles = {
            let mut handles = self.handles.write().await;
            handles
                .drain()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>()
        };

        for handle in handles {
            stop_listener(handle).await;
        }
    }
}

/// Extract the port number from an address string like "0.0.0.0:80".
fn extract_port(addr: &str) -> Option<u16> {
    addr.rsplit(':').next().and_then(|p| p.parse().ok())
}

#[derive(Clone)]
struct EffectiveTlsListener {
    listen: String,
    default_certificate: String,
    sni_certificates: HashMap<String, String>,
}

#[derive(Debug)]
struct SniOrDefaultResolver {
    default_key: Arc<rustls::sign::CertifiedKey>,
    by_name: HashMap<String, Arc<rustls::sign::CertifiedKey>>,
}

impl rustls::server::ResolvesServerCert for SniOrDefaultResolver {
    fn resolve(
        &self,
        client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        client_hello
            .server_name()
            .and_then(|name| self.by_name.get(name).cloned())
            .or_else(|| Some(self.default_key.clone()))
    }
}

fn certified_key(
    config: &AppConfig,
    certificate_name: &str,
) -> anyhow::Result<rustls::sign::CertifiedKey> {
    crate::install_rustls_crypto_provider();

    let certificate = config
        .certificates
        .iter()
        .find(|cert| cert.name == certificate_name)
        .with_context(|| format!("certificate '{certificate_name}' not found"))?;
    let certs = parse_cert_chain(&certificate.cert)
        .with_context(|| format!("invalid certificate '{}'", certificate.name))?;
    let key = parse_private_key(&certificate.key)
        .with_context(|| format!("invalid private key for certificate '{}'", certificate.name))?;
    let certified_key = rustls::sign::CertifiedKey::from_der(
        certs,
        key,
        rustls::crypto::CryptoProvider::get_default()
            .ok_or_else(|| anyhow::anyhow!("no rustls crypto provider installed"))?,
    )
    .context("certificate and private key do not match")?;
    Ok(certified_key)
}

fn tls_acceptor(
    config: &AppConfig,
    listener: &EffectiveTlsListener,
) -> anyhow::Result<TlsAcceptor> {
    let default_key = Arc::new(certified_key(config, &listener.default_certificate)?);
    let mut by_name = HashMap::new();
    for (host, certificate) in &listener.sni_certificates {
        by_name.insert(host.clone(), Arc::new(certified_key(config, certificate)?));
    }
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(SniOrDefaultResolver {
            default_key,
            by_name,
        }));
    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

pub(crate) fn validate_tls_config(config: &AppConfig) -> anyhow::Result<()> {
    validate_match_sets(config)?;
    for rule in config.rules.iter().filter(|rule| rule_tls_enabled(rule)) {
        if rule.listen.as_deref().unwrap_or("").trim().is_empty() {
            anyhow::bail!("TLS rule '{}' must define a listen address", rule.id);
        }
        let certificate = rule
            .tls
            .as_ref()
            .map(|tls| tls.certificate.as_str())
            .unwrap_or("");
        certified_key(config, certificate)
            .with_context(|| format!("invalid TLS certificate for rule '{}'", rule.id))?;
    }
    validate_listener_protocol_conflicts(config)?;
    for listener in effective_tls_listeners(config) {
        tls_acceptor(config, &listener)
            .with_context(|| format!("invalid TLS listener {}", listener.listen))?;
    }
    Ok(())
}

fn validate_match_sets(config: &AppConfig) -> anyhow::Result<()> {
    let mut names = std::collections::HashSet::new();
    for set in &config.match_sets {
        let name = set.name.trim();
        if name.is_empty() {
            anyhow::bail!("match set name is required");
        }
        if !names.insert(name.to_string()) {
            anyhow::bail!("match set '{}' already exists", name);
        }
        if set.conditions.is_none() {
            anyhow::bail!("match set '{}' must define conditions", name);
        }
        if contains_route_condition(set.conditions.as_ref()) {
            anyhow::bail!(
                "match set '{}' cannot contain host or path conditions; configure host and location on the route rule",
                name
            );
        }
    }
    for rule in &config.rules {
        if let Some(name) = rule
            .match_set
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            if !names.contains(name) {
                anyhow::bail!("rule '{}' references missing match set '{}'", rule.id, name);
            }
        }
        validate_rule_route_dimensions(rule)?;
        if contains_route_condition(rule.conditions.as_ref()) {
            anyhow::bail!(
                "rule '{}' cannot contain host or path conditions; configure host and location on the route rule",
                rule.id
            );
        }
    }
    Ok(())
}

fn validate_rule_route_dimensions(rule: &Rule) -> anyhow::Result<()> {
    match rule.host.match_type {
        HostMatchType::Any => {}
        HostMatchType::Exact => {
            let value = rule.host.value.as_deref().unwrap_or("").trim();
            if value.is_empty() {
                anyhow::bail!("rule '{}' exact host requires a value", rule.id);
            }
            if value.contains('/') {
                anyhow::bail!("rule '{}' host cannot contain a path", rule.id);
            }
        }
        HostMatchType::Wildcard => {
            let value = rule.host.value.as_deref().unwrap_or("").trim();
            if !value.starts_with("*.") || value[2..].is_empty() || value[2..].contains('*') {
                anyhow::bail!(
                    "rule '{}' wildcard host must use '*.example.com' format",
                    rule.id
                );
            }
        }
    }

    match rule.location.match_type {
        LocationMatchType::Exact | LocationMatchType::Prefix => {
            if !rule.location.value.starts_with('/') {
                anyhow::bail!("rule '{}' location must start with '/'", rule.id);
            }
        }
        LocationMatchType::Regex => {
            regex::Regex::new(&rule.location.value)
                .with_context(|| format!("rule '{}' has invalid location regex", rule.id))?;
        }
    }
    Ok(())
}

fn contains_route_condition(expr: Option<&ConditionExpr>) -> bool {
    match expr {
        Some(ConditionExpr::Leaf {
            condition_type: ConditionType::Host | ConditionType::Path,
            ..
        }) => true,
        Some(ConditionExpr::And { children }) | Some(ConditionExpr::Or { children }) => children
            .iter()
            .any(|child| contains_route_condition(Some(child))),
        _ => false,
    }
}

fn rule_tls_enabled(rule: &Rule) -> bool {
    rule.tls.as_ref().is_some_and(|tls| tls.enabled)
}

fn validate_listener_protocol_conflicts(config: &AppConfig) -> anyhow::Result<()> {
    config.validate_tcp_listeners()?;
    let mut occupied_ports: HashMap<u16, String> = HashMap::new();
    if let Some(port) = extract_port(&config.proxy_listen) {
        occupied_ports.insert(port, config.proxy_listen.clone());
    }

    for rule in config.rules.iter().filter(|rule| !rule_tls_enabled(rule)) {
        let Some(listen) = rule.listen.as_ref() else {
            continue;
        };
        if let Some(port) = extract_port(listen) {
            occupied_ports.entry(port).or_insert_with(|| listen.clone());
        }
    }

    for listener in effective_tls_listeners(config) {
        let Some(port) = extract_port(&listener.listen) else {
            continue;
        };
        if let Some(existing_listen) = occupied_ports.get(&port) {
            anyhow::bail!(
                "HTTPS listener ({}) conflicts with listener ({}) on port {}",
                listener.listen,
                existing_listen,
                port
            );
        }
        occupied_ports.insert(port, listener.listen);
    }

    for listener in &config.tcp_listeners {
        let Some(port) = extract_port(&listener.listen) else {
            continue;
        };
        if let Some(existing_listen) = occupied_ports.get(&port) {
            anyhow::bail!(
                "TCP listener ({}) conflicts with listener ({}) on port {}",
                listener.listen,
                existing_listen,
                port
            );
        }
        occupied_ports.insert(port, listener.listen.clone());
    }

    Ok(())
}

fn proxy_listener_specs(config: &AppConfig) -> anyhow::Result<HashMap<String, ListenerSpec>> {
    validate_listener_protocol_conflicts(config)?;

    let mut specs = HashMap::new();
    specs.insert(
        config.proxy_listen.clone(),
        ListenerSpec {
            listen: config.proxy_listen.clone(),
            protocol: ListenerProtocol::Http,
            signature: String::new(),
            acceptor: None,
            tcp_listener: None,
        },
    );

    for rule in config.rules.iter().filter(|rule| !rule_tls_enabled(rule)) {
        let Some(listen) = rule
            .listen
            .as_ref()
            .filter(|listen| !listen.trim().is_empty())
        else {
            continue;
        };
        specs.entry(listen.clone()).or_insert_with(|| ListenerSpec {
            listen: listen.clone(),
            protocol: ListenerProtocol::Http,
            signature: String::new(),
            acceptor: None,
            tcp_listener: None,
        });
    }

    for listener in effective_tls_listeners(config) {
        specs.insert(
            listener.listen.clone(),
            ListenerSpec {
                signature: tls_listener_signature(config, &listener),
                acceptor: Some(tls_acceptor(config, &listener)?),
                protocol: ListenerProtocol::Https,
                listen: listener.listen,
                tcp_listener: None,
            },
        );
    }

    for listener in &config.tcp_listeners {
        specs.insert(
            listener.listen.clone(),
            ListenerSpec {
                signature: tcp_listener_signature(config, listener),
                acceptor: None,
                protocol: ListenerProtocol::Tcp,
                listen: listener.listen.clone(),
                tcp_listener: Some(listener.clone()),
            },
        );
    }

    Ok(specs)
}

fn tcp_listener_signature(config: &AppConfig, listener: &TcpListenerConfig) -> String {
    let mut sni_routes: Vec<_> = listener.sni_routes.iter().collect();
    sni_routes.sort_by_key(|(host, _)| *host);
    let sni_routes = sni_routes
        .into_iter()
        .map(|(host, upstream)| {
            format!(
                "{host}={upstream}:{}",
                upstream_fingerprint(config, upstream)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let upstream = listener.upstream.as_deref().unwrap_or_default();
    format!(
        "mode={:?};upstream={}:{};sni={};maxconn={:?};timeouts={}",
        listener.mode,
        upstream,
        upstream_fingerprint(config, upstream),
        sni_routes,
        listener.maxconn,
        serde_json::to_string(&config.timeouts).unwrap_or_default()
    )
}

fn upstream_fingerprint(config: &AppConfig, upstream_name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    upstream_name.hash(&mut hasher);
    if let Some(upstream) = config.upstreams.get(upstream_name) {
        serde_json::to_string(upstream)
            .unwrap_or_default()
            .hash(&mut hasher);
    }
    hasher.finish()
}

fn tls_listener_signature(config: &AppConfig, listener: &EffectiveTlsListener) -> String {
    let mut sni: Vec<_> = listener.sni_certificates.iter().collect();
    sni.sort_by_key(|(host, _)| *host);
    let sni = sni
        .into_iter()
        .map(|(host, certificate)| {
            format!(
                "{host}={certificate}:{}",
                certificate_fingerprint(config, certificate)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "default={}:{};sni={}",
        listener.default_certificate,
        certificate_fingerprint(config, &listener.default_certificate),
        sni
    )
}

fn certificate_fingerprint(config: &AppConfig, name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    if let Some(certificate) = config.certificates.iter().find(|cert| cert.name == name) {
        certificate.cert.hash(&mut hasher);
        certificate.key.hash(&mut hasher);
    }
    hasher.finish()
}

fn effective_tls_listeners(config: &AppConfig) -> Vec<EffectiveTlsListener> {
    let mut listeners: HashMap<String, EffectiveTlsListener> = HashMap::new();

    for rule in config.rules.iter().filter(|rule| rule_tls_enabled(rule)) {
        let Some(listen) = rule
            .listen
            .as_ref()
            .filter(|listen| !listen.trim().is_empty())
        else {
            continue;
        };
        let Some(tls) = rule.tls.as_ref() else {
            continue;
        };
        let entry = listeners
            .entry(listen.clone())
            .or_insert_with(|| EffectiveTlsListener {
                listen: listen.clone(),
                default_certificate: tls.certificate.clone(),
                sni_certificates: HashMap::new(),
            });
        if entry.default_certificate.trim().is_empty() {
            entry.default_certificate = tls.certificate.clone();
        }
        for host in route_host_values(rule) {
            entry
                .sni_certificates
                .insert(host.to_ascii_lowercase(), tls.certificate.clone());
        }
    }

    for legacy in config
        .tls_listeners
        .iter()
        .filter(|listener| listener.enabled)
    {
        listeners
            .entry(legacy.listen.clone())
            .or_insert_with(|| EffectiveTlsListener {
                listen: legacy.listen.clone(),
                default_certificate: legacy.certificate.clone(),
                sni_certificates: HashMap::new(),
            });
    }

    listeners.into_values().collect()
}

fn route_host_values(rule: &Rule) -> Vec<String> {
    match rule.host.match_type {
        HostMatchType::Exact | HostMatchType::Wildcard => rule
            .host
            .value
            .as_deref()
            .map(strip_host_port)
            .into_iter()
            .collect(),
        HostMatchType::Any => Vec::new(),
    }
}

fn strip_host_port(host: &str) -> String {
    host.split(':')
        .next()
        .unwrap_or(host)
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn parse_cert_chain(
    input: &str,
) -> anyhow::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let trimmed = input.trim();
    if looks_like_file_path(trimmed) {
        let bytes = std::fs::read(trimmed)
            .with_context(|| format!("failed to read certificate file {trimmed}"))?;
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if text.contains("-----BEGIN") {
                return parse_cert_chain(text);
            }
        }
        return Ok(vec![rustls::pki_types::CertificateDer::from(bytes)]);
    }
    if trimmed.contains("-----BEGIN") {
        let certs = rustls::pki_types::CertificateDer::pem_slice_iter(trimmed.as_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        if certs.is_empty() {
            anyhow::bail!("no PEM certificate blocks found");
        }
        return Ok(certs);
    }

    let der = general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .context("DER certificate must be base64 encoded")?;
    Ok(vec![rustls::pki_types::CertificateDer::from(der)])
}

fn parse_private_key(input: &str) -> anyhow::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let trimmed = input.trim();
    if looks_like_file_path(trimmed) {
        let bytes = std::fs::read(trimmed)
            .with_context(|| format!("failed to read private key file {trimmed}"))?;
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if text.contains("-----BEGIN") {
                return parse_private_key(text);
            }
        }
        return rustls::pki_types::PrivateKeyDer::try_from(bytes)
            .map_err(|_| anyhow::anyhow!("unsupported DER private key format"));
    }
    if trimmed.contains("-----BEGIN") {
        return rustls::pki_types::PrivateKeyDer::from_pem_slice(trimmed.as_bytes())
            .context("no supported PEM private key block found");
    }

    let der = general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .context("DER private key must be base64 encoded")?;
    rustls::pki_types::PrivateKeyDer::try_from(der)
        .map_err(|_| anyhow::anyhow!("unsupported DER private key format"))
}

fn looks_like_file_path(input: &str) -> bool {
    input.starts_with('/') || input.starts_with("./") || input.starts_with("../")
}

async fn serve_tls(
    listener: TcpListener,
    app: Router,
    acceptor: TlsAcceptor,
    drain: DrainController,
    mut shutdown: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    loop {
        let (stream, remote_addr) = tokio::select! {
            result = listener.accept() => result?,
            _ = &mut shutdown => break,
        };
        let Some(connection_lease) = drain.try_acquire() else {
            break;
        };
        let app = app.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            let _connection_lease = connection_lease;
            let tls_stream = match acceptor.accept(stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(%remote_addr, %error, "TLS handshake failed");
                    return;
                }
            };
            let service =
                hyper_util::service::TowerToHyperService::new(app.layer(Extension(remote_addr)));
            if let Err(error) =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection_with_upgrades(
                        hyper_util::rt::TokioIo::new(tls_stream),
                        service,
                    )
                    .await
            {
                tracing::warn!(%remote_addr, %error, "TLS proxy connection failed");
            }
        });
    }
    Ok(())
}

async fn start_listener(state: AppState, spec: ListenerSpec) -> anyhow::Result<ListenerHandle> {
    let listener = TcpListener::bind(&spec.listen)
        .await
        .with_context(|| format!("failed to bind proxy listener to {}", spec.listen))?;
    let drain = DrainController::default();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let listen = spec.listen.clone();
    let protocol = spec.protocol;
    let join = match spec.protocol {
        ListenerProtocol::Http => {
            let app = proxy_router(state, Some(spec.listen.clone()), Some(drain.clone()));
            tokio::spawn(async move {
                if let Err(error) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                {
                    tracing::error!(addr = %listen, %error, "HTTP proxy listener failed");
                }
            })
        }
        ListenerProtocol::Https => {
            let app = proxy_router(state, Some(spec.listen.clone()), Some(drain.clone()));
            let acceptor = spec
                .acceptor
                .clone()
                .ok_or_else(|| anyhow::anyhow!("HTTPS listener missing TLS acceptor"))?;
            let listener_drain = drain.clone();
            tokio::spawn(async move {
                if let Err(error) =
                    serve_tls(listener, app, acceptor, listener_drain, shutdown_rx).await
                {
                    tracing::error!(addr = %listen, %error, "HTTPS proxy listener failed");
                }
            })
        }
        ListenerProtocol::Tcp => {
            let listener_config = spec
                .tcp_listener
                .clone()
                .ok_or_else(|| anyhow::anyhow!("TCP listener missing config"))?;
            let runtime: Arc<dyn TcpRuntime> = Arc::new(SharedTcpRuntime {
                proxy_runtime: state.proxy_runtime.clone(),
            });
            let metrics = state.metrics.clone();
            let listener_drain = drain.clone();
            tokio::spawn(async move {
                if let Err(error) = run_tcp_listener(
                    listener,
                    listener_config,
                    runtime,
                    metrics,
                    listener_drain,
                    shutdown_rx,
                )
                .await
                {
                    tracing::error!(addr = %listen, %error, "TCP proxy listener failed");
                }
            })
        }
    };
    tracing::info!(listen = %spec.listen, protocol = ?protocol, "proxy listener started");
    Ok(ListenerHandle {
        spec,
        shutdown: shutdown_tx,
        join,
        drain,
    })
}

async fn stop_listener(mut handle: ListenerHandle) {
    handle.drain.start_draining();
    let _ = handle.shutdown.send(());
    let deadline = Instant::now() + LISTENER_DRAIN_HARD_TIMEOUT;

    if tokio::time::timeout(LISTENER_DRAIN_HARD_TIMEOUT, &mut handle.join)
        .await
        .is_err()
    {
        tracing::warn!(
            listen = %handle.spec.listen,
            "proxy listener did not stop before hard drain timeout; aborting listener task"
        );
        handle.join.abort();
        let _ = handle.join.await;
        return;
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    if !handle.drain.wait_empty(remaining).await {
        tracing::warn!(
            listen = %handle.spec.listen,
            active = handle.drain.active(),
            "proxy listener drain timed out with active leases"
        );
    }
}

fn proxy_router(
    state: AppState,
    listen_addr: Option<String>,
    drain: Option<DrainController>,
) -> Router {
    let router = Router::new()
        .fallback(any(proxy_handler))
        .layer(TraceLayer::new_for_http());
    let router = if let Some(listen_addr) = listen_addr {
        router.layer(Extension(listen_addr))
    } else {
        router
    };
    let router = if let Some(drain) = drain {
        router.layer(Extension(drain))
    } else {
        router
    };
    Router::new().merge(router).with_state(state)
}

/// API + Admin UI router (no proxy fallback).
fn api_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/auth/login", put(handlers::login))
        .route("/api/auth/setup-status", get(handlers::setup_status))
        .route("/api/auth/setup", post(handlers::setup))
        .route("/api/version", get(handlers::version))
        .route("/api/health", get(handlers::health))
        .route("/metrics", get(handlers::metrics));

    let protected = Router::new()
        .route(
            "/api/config",
            get(handlers::get_config).put(handlers::put_config),
        )
        .route("/api/certificates", post(handlers::upload_certificate))
        .route(
            "/api/match-sets",
            get(handlers::list_match_sets).post(handlers::create_match_set),
        )
        .route(
            "/api/match-sets/:name",
            put(handlers::update_match_set).delete(handlers::delete_match_set),
        )
        .route(
            "/api/rules",
            get(handlers::list_rules).post(handlers::create_rule),
        )
        .route(
            "/api/rules/:id",
            put(handlers::update_rule).delete(handlers::delete_rule),
        )
        .route(
            "/api/upstreams",
            get(handlers::list_upstreams).post(handlers::create_upstream),
        )
        .route("/api/upstream-health", get(handlers::upstream_health))
        .route("/api/runtime/upstreams", get(runtime_api::list_upstreams))
        .route(
            "/api/runtime/upstreams/:upstream/targets/enable",
            post(runtime_api::enable_target),
        )
        .route(
            "/api/runtime/upstreams/:upstream/targets/disable",
            post(runtime_api::disable_target),
        )
        .route(
            "/api/runtime/upstreams/:upstream/targets/drain",
            post(runtime_api::drain_target),
        )
        .route(
            "/api/runtime/upstreams/:upstream/targets/weight",
            post(runtime_api::set_target_weight),
        )
        .route(
            "/api/monitoring/query-range",
            get(handlers::prometheus_query_range),
        )
        .route(
            "/api/upstreams/:id",
            put(handlers::update_upstream).delete(handlers::delete_upstream),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_mw::require_auth,
        ));

    Router::new()
        .merge(public)
        .merge(protected)
        .route("/admin", any(super::ui::serve_admin_ui))
        .route("/admin/", any(super::ui::serve_admin_ui))
        .route("/admin/*path", any(super::ui::serve_admin_ui))
        // API port returns 404 for unknown /api/ paths instead of proxying
        .fallback(api_not_found)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn api_not_found(
    _request: Request<Body>,
) -> Result<Response<Body>, std::convert::Infallible> {
    Ok(StatusCode::NOT_FOUND.into_response())
}

fn client_ip_string(addr: std::net::SocketAddr) -> String {
    addr.ip().to_string()
}

fn normalize_host_key(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if let Some(stripped) = host.strip_suffix(":80") {
        stripped.to_string()
    } else if let Some(stripped) = host.strip_suffix(":443") {
        stripped.to_string()
    } else {
        host
    }
}

async fn proxy_handler(
    State(state): State<AppState>,
    listen_addr: Option<Extension<String>>,
    drain: Option<Extension<DrainController>>,
    remote_addr: Option<Extension<std::net::SocketAddr>>,
    connect_info: Option<ConnectInfo<std::net::SocketAddr>>,
    request: Request<Body>,
) -> Result<Response<Body>, std::convert::Infallible> {
    let drain_lease = match drain {
        Some(Extension(drain)) => match drain.try_acquire() {
            Some(lease) => Some(lease),
            None => return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response()),
        },
        None => None,
    };
    let source = remote_addr
        .map(|Extension(addr)| client_ip_string(addr))
        .or_else(|| connect_info.map(|ConnectInfo(addr)| client_ip_string(addr)))
        .unwrap_or_else(|| "-".to_string());
    let listen_addr = listen_addr.map(|Extension(addr)| addr).or_else(|| {
        request
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .map(|h| {
                if h.contains(':') {
                    format!("0.0.0.0:{}", h.rsplit(':').next().unwrap_or(""))
                } else {
                    h.to_string()
                }
            })
    });

    let request_path = request.uri().path().to_string();
    let (
        config,
        clients,
        access_logger,
        target_base,
        timeout_policy,
        metric_labels,
        header_policy,
        path_actions,
        limit_policy,
        limit_context,
        limit_state,
        retry_policy,
        balancer,
        balance_client_ip,
        balance_path,
        target_lease,
    ) = {
        let runtime = state.proxy_runtime.load();
        let config = runtime.config.clone();
        let clients = runtime.clients.clone();
        let access_logger = runtime.access_logger.clone();
        let limit_state = runtime.limits.clone();
        let balancer = runtime.balancer.clone();
        let match_request = request_for_matching(&request);
        let request_host = normalize_host_key(
            request
                .headers()
                .get("host")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("-"),
        );

        let selected = listen_addr.as_deref().and_then(|addr| {
            runtime
                .matcher
                .match_request(&match_request, Some(addr))
                .and_then(|rule| {
                    let rule_label = if rule.name.trim().is_empty() {
                        rule.id.clone()
                    } else {
                        rule.name.clone()
                    };
                    runtime
                        .balancer
                        .select(
                            &rule.upstream,
                            BalanceContext {
                                client_ip: Some(source.as_str()),
                                path: &request_path,
                            },
                        )
                        .map(|target| {
                            let timeout_policy =
                                resolve_proxy_timeout_policy(&config, rule, &target.url);
                            let limit_policy = limit_policy_with_resolved_queue_timeout(
                                rule.limit_policy.clone(),
                                &timeout_policy,
                            );
                            (
                                rule_label,
                                rule.upstream.clone(),
                                timeout_policy,
                                rule.header_policy.clone(),
                                rule.path_actions.clone(),
                                limit_policy,
                                config
                                    .upstreams
                                    .get(&rule.upstream)
                                    .map(|upstream| upstream.retry.clone())
                                    .unwrap_or_default(),
                                LimitContext {
                                    listen: addr.to_string(),
                                    rule: rule.id.clone(),
                                    client_ip: source.clone(),
                                    host: request_host.clone(),
                                },
                                target.url,
                                target.active_connection,
                            )
                        })
                })
        });

        let listen_label = listen_addr
            .clone()
            .unwrap_or_else(|| config.proxy_listen.clone());
        let (
            target_base,
            timeout_policy,
            metric_labels,
            header_policy,
            path_actions,
            limit_policy,
            retry_policy,
            limit_context,
            target_lease,
        ) = match selected {
            Some((
                rule,
                upstream,
                timeout_policy,
                header_policy,
                path_actions,
                limit_policy,
                retry_policy,
                limit_context,
                target,
                target_lease,
            )) => (
                target,
                timeout_policy,
                ProxyMetricLabels {
                    listen: listen_label,
                    rule,
                    upstream,
                },
                header_policy,
                path_actions,
                limit_policy,
                retry_policy,
                Some(limit_context),
                Some(target_lease),
            ),
            None => (
                config.fallback.url.clone(),
                ResolvedTimeoutPolicy::resolve(&config.timeouts, None, None, None),
                ProxyMetricLabels::fallback(listen_label),
                Default::default(),
                Vec::new(),
                Default::default(),
                Default::default(),
                None,
                None,
            ),
        };
        (
            config,
            clients,
            access_logger,
            target_base,
            timeout_policy,
            metric_labels,
            header_policy,
            path_actions,
            limit_policy,
            limit_context,
            limit_state,
            retry_policy,
            balancer,
            source.clone(),
            request_path,
            target_lease,
        )
    };

    handle_proxy_with_target(
        request,
        config,
        clients,
        target_base,
        Some(state.metrics),
        access_logger,
        ProxyRequestContext {
            access: ProxyAccessLogContext { source },
            metric_labels,
            timeout_policy,
            header_policy,
            path_actions,
            limit_state: Some(limit_state),
            limit_context,
            limit_policy,
            retry_policy,
            balancer: Some(balancer),
            balance_client_ip,
            balance_path,
            target_lease,
            drain_lease,
        },
    )
    .await
}

fn resolve_proxy_timeout_policy(
    config: &AppConfig,
    rule: &Rule,
    target_url: &str,
) -> ResolvedTimeoutPolicy {
    let upstream = config.upstreams.get(&rule.upstream);
    let target = upstream.and_then(|upstream| {
        upstream
            .targets
            .iter()
            .find(|target| target.url == target_url)
    });
    let mut rule_timeouts = rule.timeouts.clone();
    if rule_timeouts.server_timeout_seconds.is_none() && rule.request_timeout > 0 {
        rule_timeouts.server_timeout_seconds = Some(rule.request_timeout);
    }

    ResolvedTimeoutPolicy::resolve(
        &config.timeouts,
        Some(&rule_timeouts),
        upstream.map(|upstream| &upstream.timeouts),
        target.map(|target| &target.timeouts),
    )
}

fn limit_policy_with_resolved_queue_timeout(
    mut policy: LimitPolicy,
    timeout_policy: &ResolvedTimeoutPolicy,
) -> LimitPolicy {
    if policy.queue_timeout_ms.is_none() {
        policy.queue_timeout_ms = Some(duration_millis_u64(timeout_policy.queue_timeout));
    }
    policy
}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub fn routes(state: AppState) -> Router {
    api_router(state)
}

pub async fn run(config: AppConfig, db: Database) -> anyhow::Result<()> {
    run_until_shutdown(config, db, std::future::pending::<()>()).await
}

pub async fn run_until_shutdown<S>(
    config: AppConfig,
    db: Database,
    shutdown: S,
) -> anyhow::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let api_listen = config.listen.clone();
    let jwt_secret = db.ensure_jwt_secret()?;

    // Validate that API and proxy ports don't conflict
    let api_port = extract_port(&api_listen);
    for spec in proxy_listener_specs(&config)?.values() {
        if api_port.is_some() && extract_port(&spec.listen) == api_port {
            anyhow::bail!(
                "API listen ({}) and proxy listen ({}) must use different ports",
                api_listen,
                spec.listen
            );
        }
    }

    let health = HealthRegistry::new();
    let health_config = ConfigSnapshot::new();
    health_config.update(&config.upstreams);
    let runtime_state = RuntimeState::default();

    let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
        matcher: Arc::new(Matcher::new_verified_with_match_sets(
            config.rules.clone(),
            config.match_sets.clone(),
            jwt_secret.clone(),
        )),
        balancer: Arc::new(Balancer::new_with_runtime(
            config.upstreams.clone(),
            Some(health.clone()),
            runtime_state.clone(),
        )),
        config: Arc::new(config.clone()),
        clients: Arc::new(ProxyClients::new(
            if config.connect_timeout > 0 {
                Some(std::time::Duration::from_secs(config.connect_timeout))
            } else {
                None
            },
            config.pool_max_idle_per_host,
            if config.pool_idle_timeout > 0 {
                Some(std::time::Duration::from_secs(config.pool_idle_timeout))
            } else {
                None
            },
            if config.tcp_keepalive > 0 {
                Some(std::time::Duration::from_secs(config.tcp_keepalive))
            } else {
                None
            },
        )),
        access_logger: AccessLogger::from_config(&config.access_log).map(Arc::new),
        limits: Arc::new(LimitState::default()),
    }));

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        db: Arc::new(db),
        jwt_secret: Arc::new(jwt_secret),
        metrics: Arc::new(ProxyMetrics::new().context("failed to initialize metrics")?),
        health,
        health_config,
        proxy_runtime,
        runtime_state,
        listener_manager: Arc::new(ListenerManager::default()),
        listener_lifecycle: Arc::new(Mutex::new(())),
        shutting_down: Arc::new(AtomicBool::new(false)),
    };

    let initial_config = state.config.read().await.clone();
    state.sync_proxy_listeners(&initial_config).await?;

    // Background task to reload config from DB every 5 seconds
    spawn_config_reloader(state.clone());
    spawn_health_checker(state.clone());
    let (api_shutdown_tx, api_shutdown_rx) = oneshot::channel();
    tokio::spawn({
        let state = state.clone();
        async move {
            shutdown.await;
            state.shutdown_proxy_listeners().await;
            let _ = api_shutdown_tx.send(());
        }
    });

    // API listener (main, blocking — keeps process alive)
    let app = api_router(state.clone());
    let listener = TcpListener::bind(&api_listen)
        .await
        .with_context(|| format!("failed to bind API server to {api_listen}"))?;

    tracing::info!(%api_listen, "API server listening");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = api_shutdown_rx.await;
        })
        .await
        .context("API server failed");

    state.shutdown_proxy_listeners().await;
    result?;

    Ok(())
}

fn spawn_config_reloader(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if state.shutting_down.load(Ordering::Acquire) {
                break;
            }
            match state.db.load_config() {
                Ok(new_config) => {
                    let old_config = {
                        let config = state.config.read().await;
                        config.clone()
                    };
                    if old_config == new_config {
                        continue;
                    }
                    if let Err(error) = validate_tls_config(&new_config) {
                        tracing::warn!(%error, "reloaded config rejected by validation");
                        continue;
                    }
                    if let Err(error) = state.sync_proxy_listeners(&new_config).await {
                        tracing::warn!(%error, "reloaded config rejected while syncing listeners");
                        continue;
                    }
                    {
                        let mut config = state.config.write().await;
                        *config = new_config.clone();
                    }
                    state.rebuild_proxy_runtime(&old_config, &new_config);
                }
                Err(e) => {
                    tracing::warn!("failed to reload config from DB: {e}");
                }
            }
        }
    });
}

fn spawn_health_checker(state: AppState) {
    let clients = state.proxy_runtime.load().clients.clone();
    tokio::spawn(async move {
        run_health_checks(state.health.clone(), state.health_config.clone(), clients).await;
    });
}

#[cfg(test)]
mod tests {
    use super::{
        client_ip_string, normalize_host_key, AppState, ConfigSnapshot, LimitState,
        ListenerProtocol, Matcher, ProxyClients, ProxyRuntime,
    };
    use crate::config::yaml::{AppConfig, Fallback, TcpListenerConfig, TcpListenerMode};
    use crate::db::Database;
    use crate::models::{BalanceAlgorithm, LimitPolicy, Target, Upstream};
    use crate::observability::metrics::ProxyMetrics;
    use crate::proxy::balancer::{BalanceContext, Balancer};
    use crate::proxy::health::HealthRegistry;
    use crate::runtime::state::{RuntimeState, TargetKey};
    use crate::runtime::timeouts::{ResolvedTimeoutPolicy, TimeoutPolicy};
    use arc_swap::ArcSwap;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;

    fn target(url: &str, weight: u32) -> Target {
        Target {
            url: url.to_string(),
            weight,
            timeouts: Default::default(),
        }
    }

    fn app_config_with_upstream(upstream: Upstream) -> AppConfig {
        let mut upstreams = HashMap::new();
        upstreams.insert(upstream.name.clone(), upstream);
        AppConfig {
            listen: "127.0.0.1:3000".to_string(),
            proxy_listen: "127.0.0.1:8080".to_string(),
            timeouts: Default::default(),
            limits: Default::default(),
            connect_timeout: 10,
            request_timeout: 60,
            pool_max_idle_per_host: 32,
            pool_idle_timeout: 90,
            tcp_keepalive: 60,
            certificate_dir: "/tmp".to_string(),
            access_log: Default::default(),
            monitoring: Default::default(),
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            tcp_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: Vec::new(),
            upstreams,
            fallback: Fallback {
                url: "http://127.0.0.1:9000".to_string(),
            },
        }
    }

    fn least_connections_upstream() -> Upstream {
        Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![target("http://a", 1), target("http://b", 1)],
            health_check: Default::default(),
            balance: BalanceAlgorithm::LeastConnections,
            retry: Default::default(),
            timeouts: Default::default(),
            sticky: Default::default(),
        }
    }

    #[test]
    fn client_ip_string_drops_ephemeral_port() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 54321);

        assert_eq!(client_ip_string(addr), "203.0.113.7");
    }

    #[test]
    fn normalize_host_key_lowercases_and_strips_default_ports() {
        assert_eq!(normalize_host_key("Example.COM:80"), "example.com");
        assert_eq!(normalize_host_key("Example.COM:443"), "example.com");
        assert_eq!(normalize_host_key("Example.COM:8443"), "example.com:8443");
    }

    #[test]
    fn resolved_queue_timeout_fills_limit_policy_without_overriding_explicit_value() {
        let timeout_policy = ResolvedTimeoutPolicy::resolve(
            &TimeoutPolicy {
                queue_timeout_ms: 250,
                ..Default::default()
            },
            None,
            None,
            None,
        );

        let inherited = super::limit_policy_with_resolved_queue_timeout(
            LimitPolicy::default(),
            &timeout_policy,
        );
        assert_eq!(inherited.queue_timeout_ms, Some(250));

        let explicit = super::limit_policy_with_resolved_queue_timeout(
            LimitPolicy {
                queue_timeout_ms: Some(7),
                ..Default::default()
            },
            &timeout_policy,
        );
        assert_eq!(explicit.queue_timeout_ms, Some(7));
    }

    #[test]
    fn proxy_listener_specs_include_tcp_listeners() {
        let mut config = app_config_with_upstream(least_connections_upstream());
        config.tcp_listeners = vec![TcpListenerConfig {
            name: "redis".to_string(),
            listen: "127.0.0.1:6379".to_string(),
            mode: TcpListenerMode::Tcp,
            upstream: Some("backend".to_string()),
            sni_routes: HashMap::new(),
            maxconn: Some(64),
        }];

        let specs = super::proxy_listener_specs(&config).unwrap();
        let spec = specs.get("127.0.0.1:6379").unwrap();

        assert_eq!(spec.protocol, ListenerProtocol::Tcp);
        assert_eq!(spec.tcp_listener.as_ref().unwrap().name, "redis");
    }

    #[test]
    fn proxy_listener_specs_reject_tcp_port_conflicts() {
        let mut config = app_config_with_upstream(least_connections_upstream());
        config.tcp_listeners = vec![TcpListenerConfig {
            name: "redis".to_string(),
            listen: config.proxy_listen.clone(),
            mode: TcpListenerMode::Tcp,
            upstream: Some("backend".to_string()),
            sni_routes: HashMap::new(),
            maxconn: None,
        }];

        let err = match super::proxy_listener_specs(&config) {
            Ok(_) => panic!("TCP listener conflict should fail"),
            Err(error) => error,
        };

        assert!(err.to_string().contains("conflicts"));
    }

    #[test]
    fn rebuild_proxy_runtime_preserves_balancer_runtime_state() {
        crate::install_rustls_crypto_provider();
        let config = app_config_with_upstream(least_connections_upstream());
        let health = HealthRegistry::new();
        let health_config = ConfigSnapshot::new();
        health_config.update(&config.upstreams);
        let jwt_secret = "test-secret".to_string();
        let runtime_state = RuntimeState::default();
        let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
            matcher: Arc::new(Matcher::new_verified_with_match_sets(
                config.rules.clone(),
                config.match_sets.clone(),
                jwt_secret.clone(),
            )),
            balancer: Arc::new(Balancer::new_with_runtime(
                config.upstreams.clone(),
                Some(health.clone()),
                runtime_state.clone(),
            )),
            config: Arc::new(config.clone()),
            clients: Arc::new(ProxyClients::new(None, 32, None, None)),
            access_logger: None,
            limits: Arc::new(LimitState::default()),
        }));
        let state = AppState {
            config: Arc::new(RwLock::new(config.clone())),
            db: Arc::new(Database::open_in_memory().unwrap()),
            jwt_secret: Arc::new(jwt_secret),
            metrics: Arc::new(ProxyMetrics::new().unwrap()),
            health,
            health_config,
            proxy_runtime,
            runtime_state,
            listener_manager: Arc::new(Default::default()),
            listener_lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            shutting_down: Arc::new(AtomicBool::new(false)),
        };

        let selected = state
            .proxy_runtime
            .load()
            .balancer
            .select(
                "backend",
                BalanceContext {
                    client_ip: None,
                    path: "/",
                },
            )
            .unwrap();
        assert_eq!(selected.url, "http://a");

        state.rebuild_proxy_runtime(&config, &config);

        let next = state
            .proxy_runtime
            .load()
            .balancer
            .select(
                "backend",
                BalanceContext {
                    client_ip: None,
                    path: "/",
                },
            )
            .unwrap();

        assert_eq!(next.url, "http://b");
        drop(selected);
        drop(next);
        let key = TargetKey::new("backend", "http://a");
        assert_eq!(
            state
                .proxy_runtime
                .load()
                .balancer
                .runtime_state_for_test()
                .snapshot()
                .targets[&key]
                .active_connections,
            0
        );
    }

    #[tokio::test]
    async fn sync_proxy_listeners_skips_new_listeners_after_shutdown_starts() {
        crate::install_rustls_crypto_provider();
        let config = app_config_with_upstream(least_connections_upstream());
        let health = HealthRegistry::new();
        let health_config = ConfigSnapshot::new();
        health_config.update(&config.upstreams);
        let jwt_secret = "test-secret".to_string();
        let runtime_state = RuntimeState::default();
        let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
            matcher: Arc::new(Matcher::new_verified_with_match_sets(
                config.rules.clone(),
                config.match_sets.clone(),
                jwt_secret.clone(),
            )),
            balancer: Arc::new(Balancer::new_with_runtime(
                config.upstreams.clone(),
                Some(health.clone()),
                runtime_state.clone(),
            )),
            config: Arc::new(config.clone()),
            clients: Arc::new(ProxyClients::new(None, 32, None, None)),
            access_logger: None,
            limits: Arc::new(LimitState::default()),
        }));
        let state = AppState {
            config: Arc::new(RwLock::new(config.clone())),
            db: Arc::new(Database::open_in_memory().unwrap()),
            jwt_secret: Arc::new(jwt_secret),
            metrics: Arc::new(ProxyMetrics::new().unwrap()),
            health,
            health_config,
            proxy_runtime,
            runtime_state,
            listener_manager: Arc::new(Default::default()),
            listener_lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            shutting_down: Arc::new(AtomicBool::new(true)),
        };
        let mut desired = config.clone();
        desired.proxy_listen = "127.0.0.1:0".to_string();

        state.sync_proxy_listeners(&desired).await.unwrap();

        assert!(state.listener_manager.handles.read().await.is_empty());

        state
            .listener_manager
            .sync(state.clone(), &desired)
            .await
            .unwrap();

        assert!(state.listener_manager.handles.read().await.is_empty());
    }

    #[tokio::test]
    async fn shutdown_proxy_listeners_waits_for_listener_lifecycle_lock() {
        crate::install_rustls_crypto_provider();
        let config = app_config_with_upstream(least_connections_upstream());
        let health = HealthRegistry::new();
        let health_config = ConfigSnapshot::new();
        health_config.update(&config.upstreams);
        let jwt_secret = "test-secret".to_string();
        let runtime_state = RuntimeState::default();
        let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
            matcher: Arc::new(Matcher::new_verified_with_match_sets(
                config.rules.clone(),
                config.match_sets.clone(),
                jwt_secret.clone(),
            )),
            balancer: Arc::new(Balancer::new_with_runtime(
                config.upstreams.clone(),
                Some(health.clone()),
                runtime_state.clone(),
            )),
            config: Arc::new(config.clone()),
            clients: Arc::new(ProxyClients::new(None, 32, None, None)),
            access_logger: None,
            limits: Arc::new(LimitState::default()),
        }));
        let state = AppState {
            config: Arc::new(RwLock::new(config)),
            db: Arc::new(Database::open_in_memory().unwrap()),
            jwt_secret: Arc::new(jwt_secret),
            metrics: Arc::new(ProxyMetrics::new().unwrap()),
            health,
            health_config,
            proxy_runtime,
            runtime_state,
            listener_manager: Arc::new(Default::default()),
            listener_lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            shutting_down: Arc::new(AtomicBool::new(false)),
        };

        let guard = state.listener_lifecycle.lock().await;
        let shutdown = tokio::spawn({
            let state = state.clone();
            async move {
                state.shutdown_proxy_listeners().await;
            }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!state.shutting_down.load(Ordering::Acquire));

        drop(guard);
        shutdown.await.unwrap();
        assert!(state.shutting_down.load(Ordering::Acquire));
    }
}
