use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::models::Upstream;
use crate::proxy::health::HealthRegistry;

pub struct Balancer {
    upstreams: HashMap<String, WeightedUpstream>,
    health: Option<HealthRegistry>,
}

struct WeightedUpstream {
    targets: Vec<WeightedTarget>,
    total_weight: u32,
    counter: AtomicU32,
}

struct WeightedTarget {
    url: String,
    health_key: Option<String>,
    cumulative_weight: u32,
}

impl Balancer {
    pub fn new(upstreams: HashMap<String, Upstream>) -> Self {
        Self::new_with_health(upstreams, None)
    }

    pub fn new_with_health(
        upstreams: HashMap<String, Upstream>,
        health: Option<HealthRegistry>,
    ) -> Self {
        let upstreams = upstreams
            .into_iter()
            .map(|(name, upstream)| (name, WeightedUpstream::new(upstream)))
            .collect();

        Self { upstreams, health }
    }

    pub fn select(&self, upstream_name: &str) -> Option<String> {
        let upstream = self.upstreams.get(upstream_name)?;
        if upstream.targets.is_empty() || upstream.total_weight == 0 {
            return None;
        }

        let slot = upstream.counter.fetch_add(1, Ordering::Relaxed) % upstream.total_weight;
        let idx = upstream
            .targets
            .partition_point(|target| target.cumulative_weight <= slot);

        for offset in 0..upstream.targets.len() {
            let idx = (idx + offset) % upstream.targets.len();
            let target = &upstream.targets[idx];
            if self.target_is_healthy(target) {
                return Some(target.url.clone());
            }
        }

        None
    }

    fn target_is_healthy(&self, target: &WeightedTarget) -> bool {
        let Some(health_key) = target.health_key.as_deref() else {
            return true;
        };
        self.health
            .as_ref()
            .is_none_or(|health| health.is_healthy(health_key))
    }
}

impl WeightedUpstream {
    fn new(upstream: Upstream) -> Self {
        let mut total_weight = 0u32;
        let mut targets = Vec::new();
        let upstream_name = upstream.name;
        let health_enabled = upstream.health_check.enabled;

        for target in upstream
            .targets
            .into_iter()
            .filter(|target| target.weight > 0)
        {
            let Some(next_weight) = total_weight.checked_add(target.weight) else {
                break;
            };
            total_weight = next_weight;
            let health_key =
                health_enabled.then(|| HealthRegistry::target_key(&upstream_name, &target.url));
            targets.push(WeightedTarget {
                url: target.url,
                health_key,
                cumulative_weight: total_weight,
            });
        }

        Self {
            targets,
            total_weight,
            counter: AtomicU32::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Balancer;
    use crate::models::{HealthCheck, Target, Upstream};
    use crate::proxy::health::HealthRegistry;
    use std::collections::HashMap;

    fn target(url: &str, weight: u32) -> Target {
        Target {
            url: url.to_string(),
            weight,
        }
    }

    fn upstream(name: &str, targets: Vec<Target>) -> Upstream {
        Upstream {
            name: name.to_string(),
            skip_ssl: false,
            websocket: false,
            targets,
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
        }
    }

    fn balancer_with(upstream: Upstream) -> Balancer {
        let mut upstreams = HashMap::new();
        upstreams.insert(upstream.name.clone(), upstream);
        Balancer::new(upstreams)
    }

    #[test]
    fn weighted_distribution_matches_ratio() {
        let balancer = balancer_with(upstream(
            "backend",
            vec![target("http://a", 10), target("http://b", 5)],
        ));

        let mut counts = HashMap::new();
        for _ in 0..30 {
            let selected = balancer.select("backend").unwrap();
            *counts.entry(selected).or_insert(0) += 1;
        }

        assert_eq!(counts.get("http://a"), Some(&20));
        assert_eq!(counts.get("http://b"), Some(&10));
    }

    #[test]
    fn equal_weight_targets_round_robin() {
        let balancer = balancer_with(upstream(
            "backend",
            vec![
                target("http://a", 1),
                target("http://b", 1),
                target("http://c", 1),
            ],
        ));

        let selections: Vec<String> = (0..6)
            .map(|_| balancer.select("backend").unwrap())
            .collect();

        assert_eq!(
            selections,
            vec![
                "http://a".to_string(),
                "http://b".to_string(),
                "http://c".to_string(),
                "http://a".to_string(),
                "http://b".to_string(),
                "http://c".to_string(),
            ]
        );
    }

    #[test]
    fn returns_none_for_missing_upstream() {
        let balancer = Balancer::new(HashMap::new());

        assert_eq!(balancer.select("missing"), None);
    }

    #[test]
    fn returns_none_for_upstream_without_targets() {
        let balancer = balancer_with(upstream("backend", vec![]));

        assert_eq!(balancer.select("backend"), None);
    }

    #[test]
    fn returns_none_for_upstream_without_selectable_targets() {
        let balancer = balancer_with(upstream("backend", vec![target("http://a", 0)]));

        assert_eq!(balancer.select("backend"), None);
    }

    #[test]
    fn skips_zero_weight_targets() {
        let balancer = balancer_with(upstream(
            "backend",
            vec![target("http://a", 0), target("http://b", 1)],
        ));

        for _ in 0..3 {
            assert_eq!(balancer.select("backend"), Some("http://b".to_string()));
        }
    }

    #[test]
    fn skips_unhealthy_targets_when_health_check_is_enabled() {
        let health = HealthRegistry::new();
        let check = HealthCheck {
            enabled: true,
            unhealthy_threshold: 1,
            ..Default::default()
        };
        let upstream = Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![target("http://a", 1), target("http://b", 1)],
            health_check: check.clone(),
            balance: Default::default(),
            retry: Default::default(),
        };
        health.record_probe_result(
            &HealthRegistry::target_key("backend", "http://a"),
            &check,
            false,
        );
        let mut upstreams = HashMap::new();
        upstreams.insert("backend".to_string(), upstream);
        let balancer = Balancer::new_with_health(upstreams, Some(health));

        for _ in 0..4 {
            assert_eq!(balancer.select("backend"), Some("http://b".to_string()));
        }
    }
}
