use prometheus::{
    self,
    core::{Collector, Desc},
    opts, Counter, CounterVec, Encoder, Gauge, HistogramOpts, HistogramVec, IntGauge, Opts,
    Registry, TextEncoder,
};
use std::sync::Mutex;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const REQUEST_DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0,
    300.0, 600.0, 900.0, 1800.0,
];

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
                "rustproxy_proxy_requests_total",
                "Total number of proxy requests handled."
            ),
            &["listen", "rule", "upstream", "status"],
        )?;
        registry.register(Box::new(requests_total.clone()))?;

        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                "rustproxy_proxy_request_duration_seconds",
                "Proxy request duration in seconds.",
            )
            .buckets(REQUEST_DURATION_BUCKETS_SECONDS.to_vec()),
            &["listen", "rule", "upstream"],
        )?;
        registry.register(Box::new(request_duration.clone()))?;

        let active_connections = Gauge::with_opts(opts!(
            "rustproxy_proxy_active_connections",
            "Current number of active proxy connections."
        ))?;
        registry.register(Box::new(active_connections.clone()))?;

        let config_reloads = Counter::with_opts(opts!(
            "rustproxy_proxy_config_reloads_total",
            "Total number of proxy configuration reloads."
        ))?;
        registry.register(Box::new(config_reloads.clone()))?;

        registry.register(Box::new(SelfProcessCollector::new()))?;

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

/// Collects process-level metrics (CPU, memory, FDs) for the current process.
/// Only queries the system when `/metrics` is scraped — zero overhead on the proxy hot path.
struct SelfProcessCollector {
    descs: Vec<Desc>,
    system: Mutex<System>,
    pid: Pid,
    cpu_total_seconds: Counter,
    rss_bytes: IntGauge,
    vms_bytes: IntGauge,
    open_fds: IntGauge,
    start_time: IntGauge,
}

impl SelfProcessCollector {
    fn new() -> Self {
        let pid = Pid::from(std::process::id() as usize);
        let mut descs = Vec::new();

        let cpu_total_seconds = Counter::with_opts(Opts::new(
            "rustproxy_process_cpu_seconds_total",
            "Total user and system CPU time spent in seconds.",
        ))
        .unwrap();
        descs.extend(cpu_total_seconds.desc().into_iter().cloned());

        let rss_bytes = IntGauge::with_opts(Opts::new(
            "rustproxy_process_resident_memory_bytes",
            "Resident memory size in bytes.",
        ))
        .unwrap();
        descs.extend(rss_bytes.desc().into_iter().cloned());

        let vms_bytes = IntGauge::with_opts(Opts::new(
            "rustproxy_process_virtual_memory_bytes",
            "Virtual memory size in bytes.",
        ))
        .unwrap();
        descs.extend(vms_bytes.desc().into_iter().cloned());

        let open_fds = IntGauge::with_opts(Opts::new(
            "rustproxy_process_open_fds",
            "Number of open file descriptors.",
        ))
        .unwrap();
        descs.extend(open_fds.desc().into_iter().cloned());

        let start_time = IntGauge::with_opts(Opts::new(
            "rustproxy_process_start_time_seconds",
            "Start time of the process since unix epoch in seconds.",
        ))
        .unwrap();
        descs.extend(start_time.desc().into_iter().cloned());

        Self {
            descs,
            system: Mutex::new(System::new()),
            pid,
            cpu_total_seconds,
            rss_bytes,
            vms_bytes,
            open_fds,
            start_time,
        }
    }
}

impl Collector for SelfProcessCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<prometheus::proto::MetricFamily> {
        let mut system = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => return vec![],
        };
        let pids = [self.pid];
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let proc = match system.process(self.pid) {
            Some(p) => p,
            None => return vec![],
        };

        // sysinfo reports accumulated CPU time in milliseconds. Prometheus expects seconds.
        let total_cpu_ms = proc.accumulated_cpu_time();
        let total_cpu_seconds = total_cpu_ms as f64 / 1000.0;
        let past = self.cpu_total_seconds.get();
        let delta = (total_cpu_seconds - past).max(0.0);
        if delta > 0.0 {
            self.cpu_total_seconds.inc_by(delta);
        }

        self.rss_bytes.set(proc.memory() as i64);
        self.vms_bytes.set(proc.virtual_memory() as i64);
        if let Some(fds) = proc.open_files() {
            self.open_fds.set(fds as i64);
        }
        self.start_time.set(proc.start_time() as i64);

        let mut mfs = Vec::with_capacity(5);
        mfs.extend(self.cpu_total_seconds.collect());
        mfs.extend(self.rss_bytes.collect());
        mfs.extend(self.vms_bytes.collect());
        mfs.extend(self.open_fds.collect());
        mfs.extend(self.start_time.collect());
        mfs
    }
}

#[cfg(test)]
mod tests {
    use super::ProxyMetrics;

    #[test]
    fn gathers_registered_metric_names() {
        let metrics = ProxyMetrics::new().unwrap();

        metrics.requests_total.with_label_values(&[
            "0.0.0.0:80",
            "test-rule",
            "test-upstream",
            "200",
        ]);
        metrics
            .request_duration
            .with_label_values(&["0.0.0.0:80", "test-rule", "test-upstream"]);

        let output = metrics.gather().unwrap();

        assert!(output.contains("rustproxy_proxy_requests_total"));
        assert!(output.contains("rustproxy_proxy_request_duration_seconds"));
        assert!(output.contains("le=\"1800\""));
        assert!(output.contains("rustproxy_proxy_active_connections"));
        assert!(output.contains("rustproxy_proxy_config_reloads_total"));
        assert!(output.contains("rustproxy_process_cpu_seconds_total"));
        assert!(output.contains("rustproxy_process_resident_memory_bytes"));
    }

    #[test]
    fn request_counter_increment_appears_in_gather_output() {
        let metrics = ProxyMetrics::new().unwrap();

        metrics
            .requests_total
            .with_label_values(&["0.0.0.0:80", "canary", "users", "200"])
            .inc();

        let output = metrics.gather().unwrap();

        assert!(output.contains("rustproxy_proxy_requests_total"));
        assert!(output.contains("rule=\"canary\""));
        assert!(output.contains("upstream=\"users\""));
        assert!(output.contains("status=\"200\""));
        assert!(output.contains(" 1"));
    }
}
