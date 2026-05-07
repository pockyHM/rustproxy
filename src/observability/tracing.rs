use std::sync::atomic::{AtomicU64, Ordering};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

pub fn next_request_id() -> String {
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("req-{id}")
}

#[cfg(test)]
mod tests {
    use super::next_request_id;

    #[test]
    fn request_ids_are_unique() {
        let first = next_request_id();
        let second = next_request_id();

        assert_ne!(first, second);
        assert!(first.starts_with("req-"));
        assert!(second.starts_with("req-"));
    }
}
