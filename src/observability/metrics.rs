use prometheus::{
    opts, Counter, CounterVec, Encoder, Gauge, HistogramOpts, HistogramVec, Registry, TextEncoder,
};

pub struct ProxyMetrics {
    pub registry: Registry,
    pub requests_total: CounterVec,
    pub request_duration: HistogramVec,
    pub active_connections: Gauge,
    pub config_reloads: Counter,
}

impl ProxyMetrics {
    pub fn new() -> prometheus::Result<Self> {
        let registry = Registry::new();

        let requests_total = CounterVec::new(
            opts!(
                "proxy_requests_total",
                "Total number of proxy requests handled."
            ),
            &["rule", "upstream", "status"],
        )?;
        registry.register(Box::new(requests_total.clone()))?;

        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                "proxy_request_duration_seconds",
                "Proxy request duration in seconds.",
            ),
            &["rule", "upstream"],
        )?;
        registry.register(Box::new(request_duration.clone()))?;

        let active_connections = Gauge::with_opts(opts!(
            "proxy_active_connections",
            "Current number of active proxy connections."
        ))?;
        registry.register(Box::new(active_connections.clone()))?;

        let config_reloads = Counter::with_opts(opts!(
            "proxy_config_reloads_total",
            "Total number of proxy configuration reloads."
        ))?;
        registry.register(Box::new(config_reloads.clone()))?;

        Ok(Self {
            registry,
            requests_total,
            request_duration,
            active_connections,
            config_reloads,
        })
    }

    pub fn gather(&self) -> prometheus::Result<String> {
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        encoder.encode(&metric_families, &mut buffer)?;

        String::from_utf8(buffer).map_err(|error| prometheus::Error::Msg(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::ProxyMetrics;

    #[test]
    fn gathers_registered_metric_names() {
        let metrics = ProxyMetrics::new().unwrap();

        metrics
            .requests_total
            .with_label_values(&["test-rule", "test-upstream", "200"]);
        metrics
            .request_duration
            .with_label_values(&["test-rule", "test-upstream"]);

        let output = metrics.gather().unwrap();

        assert!(output.contains("proxy_requests_total"));
        assert!(output.contains("proxy_request_duration_seconds"));
        assert!(output.contains("proxy_active_connections"));
        assert!(output.contains("proxy_config_reloads_total"));
    }

    #[test]
    fn request_counter_increment_appears_in_gather_output() {
        let metrics = ProxyMetrics::new().unwrap();

        metrics
            .requests_total
            .with_label_values(&["canary", "users", "200"])
            .inc();

        let output = metrics.gather().unwrap();

        assert!(output.contains("proxy_requests_total"));
        assert!(output.contains("rule=\"canary\""));
        assert!(output.contains("upstream=\"users\""));
        assert!(output.contains("status=\"200\""));
        assert!(output.contains(" 1"));
    }
}
