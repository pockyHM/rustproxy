use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::Arc,
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
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tower_http::trace::TraceLayer;

use crate::models::{ConditionExpr, ConditionType, HostMatchType, LocationMatchType, Rule};
use crate::{
    auth::middleware::{self as auth_mw},
    config::yaml::AppConfig,
    db::Database,
    observability::{access_log::AccessLogger, metrics::ProxyMetrics},
    proxy::balancer::{BalanceContext, Balancer},
    proxy::health::{run_health_checks, ConfigSnapshot, HealthRegistry},
    proxy::matcher::Matcher,
    proxy::request_for_matching,
    proxy::ProxyClients,
    proxy::{
        handle_proxy_with_target, ProxyAccessLogContext, ProxyMetricLabels, ProxyRequestContext,
    },
};

use super::handlers;

/// Pre-built proxy runtime shared across all requests.
/// Replaced atomically when config changes — includes clients for hot-reload.
#[derive(Clone)]
struct ProxyRuntime {
    matcher: Arc<Matcher>,
    balancer: Arc<Balancer>,
    config: Arc<AppConfig>,
    clients: Arc<ProxyClients>,
    access_logger: Option<Arc<AccessLogger>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListenerProtocol {
    Http,
    Https,
}

#[derive(Clone)]
struct ListenerSpec {
    listen: String,
    protocol: ListenerProtocol,
    signature: String,
    acceptor: Option<TlsAcceptor>,
}

struct ListenerHandle {
    spec: ListenerSpec,
    shutdown: oneshot::Sender<()>,
    join: JoinHandle<()>,
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
    listener_manager: Arc<ListenerManager>,
}

impl AppState {
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
            balancer: Arc::new(Balancer::new_with_health(
                new.upstreams.clone(),
                Some(self.health.clone()),
            )),
            config: Arc::new(new.clone()),
            clients,
            access_logger,
        };

        self.health_config.update(&new.upstreams);
        self.proxy_runtime.store(Arc::new(runtime));
    }

    pub(crate) async fn sync_proxy_listeners(&self, config: &AppConfig) -> anyhow::Result<()> {
        self.listener_manager
            .sync(self.clone(), config)
            .await
            .context("failed to sync proxy listeners")
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
            match start_listener(state.clone(), spec.clone()).await {
                Ok(handle) => {
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

            match start_listener(state.clone(), desired_spec.clone()).await {
                Ok(new_handle) => {
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
                        match start_listener(state.clone(), replaced_old_spec.clone()).await {
                            Ok(restored) => {
                                handles.insert(replaced_old_spec.listen.clone(), restored);
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
                    match start_listener(state.clone(), old_spec.clone()).await {
                        Ok(restored) => {
                            handles.insert(old_spec.listen.clone(), restored);
                        }
                        Err(restore_error) => {
                            tracing::error!(
                                listen = %old_spec.listen,
                                %restore_error,
                                "failed to restore previous listener after replacement failure"
                            );
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
    let mut http_ports: HashMap<u16, String> = HashMap::new();
    if let Some(port) = extract_port(&config.proxy_listen) {
        http_ports.insert(port, config.proxy_listen.clone());
    }

    for rule in config.rules.iter().filter(|rule| !rule_tls_enabled(rule)) {
        let Some(listen) = rule.listen.as_ref() else {
            continue;
        };
        if let Some(port) = extract_port(listen) {
            http_ports.entry(port).or_insert_with(|| listen.clone());
        }
    }

    for listener in effective_tls_listeners(config) {
        let Some(port) = extract_port(&listener.listen) else {
            continue;
        };
        if let Some(http_listen) = http_ports.get(&port) {
            anyhow::bail!(
                "HTTPS listener ({}) conflicts with HTTP listener ({}) on port {}",
                listener.listen,
                http_listen,
                port
            );
        }
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
            },
        );
    }

    Ok(specs)
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
    mut shutdown: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    loop {
        let (stream, remote_addr) = tokio::select! {
            result = listener.accept() => result?,
            _ = &mut shutdown => break,
        };
        let app = app.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
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
    let app = proxy_router(state, Some(spec.listen.clone()));
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let listen = spec.listen.clone();
    let protocol = spec.protocol;
    let join = match spec.protocol {
        ListenerProtocol::Http => tokio::spawn(async move {
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
        }),
        ListenerProtocol::Https => {
            let acceptor = spec
                .acceptor
                .clone()
                .ok_or_else(|| anyhow::anyhow!("HTTPS listener missing TLS acceptor"))?;
            tokio::spawn(async move {
                if let Err(error) = serve_tls(listener, app, acceptor, shutdown_rx).await {
                    tracing::error!(addr = %listen, %error, "HTTPS proxy listener failed");
                }
            })
        }
    };
    tracing::info!(listen = %spec.listen, protocol = ?protocol, "proxy listener started");
    Ok(ListenerHandle {
        spec,
        shutdown: shutdown_tx,
        join,
    })
}

async fn stop_listener(mut handle: ListenerHandle) {
    let _ = handle.shutdown.send(());
    tokio::select! {
        _ = &mut handle.join => {}
        _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
            handle.join.abort();
            let _ = handle.join.await;
        }
    }
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

/// Proxy-only router (no API routes).
fn proxy_router(state: AppState, listen_addr: Option<String>) -> Router {
    let router = Router::new()
        .fallback(any(proxy_handler))
        .layer(TraceLayer::new_for_http());
    let router = if let Some(listen_addr) = listen_addr {
        router.layer(Extension(listen_addr))
    } else {
        router
    };
    Router::new().merge(router).with_state(state)
}

async fn proxy_handler(
    State(state): State<AppState>,
    listen_addr: Option<Extension<String>>,
    remote_addr: Option<Extension<std::net::SocketAddr>>,
    connect_info: Option<ConnectInfo<std::net::SocketAddr>>,
    request: Request<Body>,
) -> Result<Response<Body>, std::convert::Infallible> {
    let source = remote_addr
        .map(|Extension(addr)| addr.to_string())
        .or_else(|| connect_info.map(|ConnectInfo(addr)| addr.to_string()))
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
        rule_request_timeout,
        metric_labels,
        header_policy,
        target_lease,
    ) = {
        let runtime = state.proxy_runtime.load();
        let config = runtime.config.clone();
        let clients = runtime.clients.clone();
        let access_logger = runtime.access_logger.clone();
        let match_request = request_for_matching(&request);

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
                            (
                                rule_label,
                                rule.upstream.clone(),
                                rule.request_timeout,
                                rule.header_policy.clone(),
                                target.url,
                                target.active_connection,
                            )
                        })
                })
        });

        let listen_label = listen_addr
            .clone()
            .unwrap_or_else(|| config.proxy_listen.clone());
        let (target_base, rule_request_timeout, metric_labels, header_policy, target_lease) = match selected {
            Some((rule, upstream, request_timeout, header_policy, target, target_lease)) => (
                target,
                request_timeout,
                ProxyMetricLabels {
                    listen: listen_label,
                    rule,
                    upstream,
                },
                header_policy,
                Some(target_lease),
            ),
            None => (
                config.fallback.url.clone(),
                0,
                ProxyMetricLabels::fallback(listen_label),
                Default::default(),
                None,
            ),
        };
        (
            config,
            clients,
            access_logger,
            target_base,
            rule_request_timeout,
            metric_labels,
            header_policy,
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
            request_timeout_override: rule_request_timeout,
            header_policy,
            target_lease,
        },
    )
    .await
}

pub fn routes(state: AppState) -> Router {
    api_router(state)
}

pub async fn run(config: AppConfig, db: Database) -> anyhow::Result<()> {
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

    let proxy_runtime = Arc::new(ArcSwap::from_pointee(ProxyRuntime {
        matcher: Arc::new(Matcher::new_verified_with_match_sets(
            config.rules.clone(),
            config.match_sets.clone(),
            jwt_secret.clone(),
        )),
        balancer: Arc::new(Balancer::new_with_health(
            config.upstreams.clone(),
            Some(health.clone()),
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
    }));

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        db: Arc::new(db),
        jwt_secret: Arc::new(jwt_secret),
        metrics: Arc::new(ProxyMetrics::new().context("failed to initialize metrics")?),
        health,
        health_config,
        proxy_runtime,
        listener_manager: Arc::new(ListenerManager::default()),
    };

    let initial_config = state.config.read().await.clone();
    state.sync_proxy_listeners(&initial_config).await?;

    // Background task to reload config from DB every 5 seconds
    spawn_config_reloader(state.clone());
    spawn_health_checker(state.clone());

    // API listener (main, blocking — keeps process alive)
    let app = api_router(state);
    let listener = TcpListener::bind(&api_listen)
        .await
        .with_context(|| format!("failed to bind API server to {api_listen}"))?;

    tracing::info!(%api_listen, "API server listening");
    axum::serve(listener, app)
        .await
        .context("API server failed")?;

    Ok(())
}

fn spawn_config_reloader(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
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
