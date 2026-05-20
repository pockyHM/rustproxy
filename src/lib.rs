pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod db;
pub mod models;
pub mod observability;
pub mod proxy;
pub mod runtime;
pub mod stick;
pub mod tcp;
pub mod version;

pub fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
