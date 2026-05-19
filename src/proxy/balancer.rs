use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::models::{BalanceAlgorithm, Upstream};
use crate::proxy::health::HealthRegistry;

pub struct Balancer {
    upstreams: HashMap<String, WeightedUpstream>,
    health: Option<HealthRegistry>,
}

pub struct BalanceContext<'a> {
    pub client_ip: Option<&'a str>,
    pub path: &'a str,
}

#[derive(Debug)]
pub struct SelectedTarget {
    pub url: String,
    pub upstream: String,
    pub active_connection: TargetLease,
}

#[derive(Debug)]
pub struct TargetLease {
    active_connections: Arc<AtomicU32>,
}

impl Drop for TargetLease {
    fn drop(&mut self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

struct WeightedUpstream {
    name: String,
    balance: BalanceAlgorithm,
    targets: Vec<WeightedTarget>,
    total_weight: u32,
    counter: AtomicU32,
    hash_ring: Vec<(u64, usize)>,
}

struct WeightedTarget {
    url: String,
    health_key: Option<String>,
    cumulative_weight: u32,
    active_connections: Arc<AtomicU32>,
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

    pub fn select(&self, upstream_name: &str, ctx: BalanceContext<'_>) -> Option<SelectedTarget> {
        self.select_excluding(upstream_name, ctx, None)
    }

    pub fn select_excluding(
        &self,
        upstream_name: &str,
        ctx: BalanceContext<'_>,
        excluded_url: Option<&str>,
    ) -> Option<SelectedTarget> {
        let upstream = self.upstreams.get(upstream_name)?;
        if upstream.targets.is_empty() || upstream.total_weight == 0 {
            return None;
        }

        let target = match upstream.balance {
            BalanceAlgorithm::WeightedRoundRobin => {
                self.select_weighted_round_robin(upstream, excluded_url)
            }
            BalanceAlgorithm::LeastConnections => {
                self.select_least_connections(upstream, excluded_url)
            }
            BalanceAlgorithm::IpHash => {
                let key = ctx.client_ip.unwrap_or(ctx.path);
                self.select_modulo_hash(upstream, key, excluded_url)
            }
            BalanceAlgorithm::UrlHash => self.select_modulo_hash(upstream, ctx.path, excluded_url),
            BalanceAlgorithm::ConsistentHash => {
                self.select_consistent_hash(upstream, ctx.path, excluded_url)
            }
        }?;

        target.active_connections.fetch_add(1, Ordering::Relaxed);
        Some(SelectedTarget {
            url: target.url.clone(),
            upstream: upstream.name.clone(),
            active_connection: TargetLease {
                active_connections: Arc::clone(&target.active_connections),
            },
        })
    }

    fn select_weighted_round_robin<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
    ) -> Option<&'a WeightedTarget> {
        let slot = upstream.counter.fetch_add(1, Ordering::Relaxed) % upstream.total_weight;
        let idx = upstream
            .targets
            .partition_point(|target| target.cumulative_weight <= slot);

        for offset in 0..upstream.targets.len() {
            let idx = (idx + offset) % upstream.targets.len();
            let target = &upstream.targets[idx];
            if self.target_is_healthy(target) && Some(target.url.as_str()) != excluded_url {
                return Some(target);
            }
        }

        None
    }

    fn select_least_connections<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
    ) -> Option<&'a WeightedTarget> {
        upstream
            .targets
            .iter()
            .filter(|target| {
                self.target_is_healthy(target) && Some(target.url.as_str()) != excluded_url
            })
            .min_by_key(|target| target.active_connections.load(Ordering::Relaxed))
    }

    fn select_modulo_hash<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        key: &str,
        excluded_url: Option<&str>,
    ) -> Option<&'a WeightedTarget> {
        let healthy: Vec<&WeightedTarget> = upstream
            .targets
            .iter()
            .filter(|target| {
                self.target_is_healthy(target) && Some(target.url.as_str()) != excluded_url
            })
            .collect();
        if healthy.is_empty() {
            return None;
        }
        let idx = (stable_hash(&key) as usize) % healthy.len();
        Some(healthy[idx])
    }

    fn select_consistent_hash<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        key: &str,
        excluded_url: Option<&str>,
    ) -> Option<&'a WeightedTarget> {
        if upstream.hash_ring.is_empty() {
            return self.select_modulo_hash(upstream, key, excluded_url);
        }
        let key_hash = stable_hash(&key);
        let start = upstream
            .hash_ring
            .partition_point(|(hash, _)| *hash < key_hash);
        for offset in 0..upstream.hash_ring.len() {
            let idx = (start + offset) % upstream.hash_ring.len();
            let target = &upstream.targets[upstream.hash_ring[idx].1];
            if self.target_is_healthy(target) && Some(target.url.as_str()) != excluded_url {
                return Some(target);
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
        let balance = upstream.balance;
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
                active_connections: Arc::new(AtomicU32::new(0)),
            });
        }

        let mut hash_ring = Vec::new();
        if matches!(balance, BalanceAlgorithm::ConsistentHash) {
            for (idx, target) in targets.iter().enumerate() {
                for replica in 0..128 {
                    hash_ring.push((stable_hash(&format!("{}#{replica}", target.url)), idx));
                }
            }
            hash_ring.sort_by_key(|(hash, _)| *hash);
        }

        Self {
            name: upstream_name,
            balance,
            targets,
            total_weight,
            counter: AtomicU32::new(0),
            hash_ring,
        }
    }
}

fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{BalanceContext, Balancer};
    use crate::models::{BalanceAlgorithm, HealthCheck, Target, Upstream};
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

    fn upstream_with_algorithm(
        name: &str,
        targets: Vec<Target>,
        balance: BalanceAlgorithm,
    ) -> Upstream {
        Upstream {
            balance,
            ..upstream(name, targets)
        }
    }

    fn ctx<'a>(client_ip: Option<&'a str>, path: &'a str) -> BalanceContext<'a> {
        BalanceContext { client_ip, path }
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
            let selected = balancer.select("backend", ctx(None, "/")).unwrap().url;
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
            .map(|_| balancer.select("backend", ctx(None, "/")).unwrap().url)
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

        assert!(balancer.select("missing", ctx(None, "/")).is_none());
    }

    #[test]
    fn returns_none_for_upstream_without_targets() {
        let balancer = balancer_with(upstream("backend", vec![]));

        assert!(balancer.select("backend", ctx(None, "/")).is_none());
    }

    #[test]
    fn returns_none_for_upstream_without_selectable_targets() {
        let balancer = balancer_with(upstream("backend", vec![target("http://a", 0)]));

        assert!(balancer.select("backend", ctx(None, "/")).is_none());
    }

    #[test]
    fn skips_zero_weight_targets() {
        let balancer = balancer_with(upstream(
            "backend",
            vec![target("http://a", 0), target("http://b", 1)],
        ));

        for _ in 0..3 {
            assert_eq!(
                balancer.select("backend", ctx(None, "/")).unwrap().url,
                "http://b".to_string()
            );
        }
    }

    #[test]
    fn least_connections_selects_lowest_active_target() {
        let balancer = balancer_with(upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::LeastConnections,
        ));

        let first = balancer.select("backend", ctx(None, "/")).unwrap();
        let second = balancer.select("backend", ctx(None, "/")).unwrap();

        assert_ne!(first.url, second.url);
    }

    #[test]
    fn ip_hash_maps_same_client_to_same_target() {
        let balancer = balancer_with(upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::IpHash,
        ));

        let first = balancer
            .select("backend", ctx(Some("203.0.113.7"), "/users"))
            .unwrap()
            .url;
        let second = balancer
            .select("backend", ctx(Some("203.0.113.7"), "/orders"))
            .unwrap()
            .url;

        assert_eq!(first, second);
    }

    #[test]
    fn url_hash_maps_same_path_to_same_target() {
        let balancer = balancer_with(upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::UrlHash,
        ));

        let first = balancer
            .select("backend", ctx(Some("203.0.113.7"), "/cache/item"))
            .unwrap()
            .url;
        let second = balancer
            .select("backend", ctx(Some("198.51.100.9"), "/cache/item"))
            .unwrap()
            .url;

        assert_eq!(first, second);
    }

    #[test]
    fn select_excluding_skips_previous_target() {
        let balancer = balancer_with(upstream(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
        ));

        let selected = balancer
            .select_excluding("backend", ctx(None, "/"), Some("http://a"))
            .unwrap();

        assert_eq!(selected.url, "http://b");
    }

    #[test]
    fn consistent_hash_remaps_fewer_keys_than_modulo_when_target_removed() {
        let full = balancer_with(upstream_with_algorithm(
            "backend",
            vec![
                target("http://a", 1),
                target("http://b", 1),
                target("http://c", 1),
            ],
            BalanceAlgorithm::ConsistentHash,
        ));
        let reduced = balancer_with(upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::ConsistentHash,
        ));
        let modulo_full = balancer_with(upstream_with_algorithm(
            "backend",
            vec![
                target("http://a", 1),
                target("http://b", 1),
                target("http://c", 1),
            ],
            BalanceAlgorithm::UrlHash,
        ));
        let modulo_reduced = balancer_with(upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::UrlHash,
        ));

        let keys: Vec<String> = (0..1000).map(|idx| format!("/key/{idx}")).collect();
        let consistent_remaps = keys
            .iter()
            .filter(|key| {
                full.select("backend", ctx(None, key)).unwrap().url
                    != reduced.select("backend", ctx(None, key)).unwrap().url
            })
            .count();
        let modulo_remaps = keys
            .iter()
            .filter(|key| {
                modulo_full.select("backend", ctx(None, key)).unwrap().url
                    != modulo_reduced
                        .select("backend", ctx(None, key))
                        .unwrap()
                        .url
            })
            .count();

        assert!(consistent_remaps < modulo_remaps);
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
            assert_eq!(
                balancer.select("backend", ctx(None, "/")).unwrap().url,
                "http://b".to_string()
            );
        }
    }
}
