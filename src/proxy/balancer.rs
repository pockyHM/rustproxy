use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::models::{BalanceAlgorithm, Upstream};
use crate::proxy::health::HealthRegistry;
use crate::runtime::state::{RuntimeState, TargetKey};
use crate::stick::{StickSnapshotEntry, StickTable, StickyPolicy};

pub struct Balancer {
    upstreams: HashMap<String, WeightedUpstream>,
    health: Option<HealthRegistry>,
    runtime_state: RuntimeState,
    stick_table: StickTable,
}

pub struct BalanceContext<'a> {
    pub client_ip: Option<&'a str>,
    pub path: &'a str,
    pub sticky_key: Option<&'a str>,
}

#[derive(Debug)]
pub struct SelectedTarget {
    pub url: String,
    pub upstream: String,
    pub active_connection: TargetLease,
}

#[derive(Debug)]
pub struct TargetLease {
    _runtime_lease: crate::runtime::state::TargetLease,
}

struct WeightedUpstream {
    name: String,
    balance: BalanceAlgorithm,
    sticky: StickyPolicy,
    targets: Vec<WeightedTarget>,
    total_weight: u32,
    counter: AtomicU32,
    hash_ring: Vec<(u64, usize)>,
}

struct WeightedTarget {
    url: String,
    key: TargetKey,
    weight: u32,
    health_key: Option<String>,
}

struct TargetSelection<'a> {
    target: &'a WeightedTarget,
    _runtime_lease: crate::runtime::state::TargetLease,
}

struct CandidateSet<'a> {
    enabled: Vec<&'a WeightedTarget>,
    fallback: Vec<&'a WeightedTarget>,
}

impl Balancer {
    pub fn new(upstreams: HashMap<String, Upstream>) -> Self {
        Self::new_with_health(upstreams, None)
    }

    pub fn new_with_health(
        upstreams: HashMap<String, Upstream>,
        health: Option<HealthRegistry>,
    ) -> Self {
        Self::new_with_runtime(upstreams, health, RuntimeState::default())
    }

    pub fn new_with_runtime(
        upstreams: HashMap<String, Upstream>,
        health: Option<HealthRegistry>,
        runtime_state: RuntimeState,
    ) -> Self {
        let upstreams = upstreams
            .into_iter()
            .map(|(name, upstream)| (name, WeightedUpstream::new(upstream, &runtime_state)))
            .collect();

        Self {
            upstreams,
            health,
            runtime_state,
            stick_table: StickTable::default(),
        }
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

        let sticky_key = ctx
            .sticky_key
            .filter(|key| !key.is_empty())
            .filter(|_| excluded_url.is_none() && upstream.sticky.enabled);
        if let Some(selection) = sticky_key.and_then(|key| self.select_sticky(upstream, key)) {
            let target = selection.target;
            return Some(SelectedTarget {
                url: target.url.clone(),
                upstream: upstream.name.clone(),
                active_connection: TargetLease {
                    _runtime_lease: selection._runtime_lease,
                },
            });
        }

        let candidates = match upstream.balance {
            BalanceAlgorithm::WeightedRoundRobin => {
                self.weighted_round_robin_candidates(upstream, excluded_url)
            }
            BalanceAlgorithm::LeastConnections => {
                self.least_connections_candidates(upstream, excluded_url)
            }
            BalanceAlgorithm::IpHash => {
                let key = ctx.client_ip.unwrap_or(ctx.path);
                self.modulo_hash_candidates(upstream, key, excluded_url)
            }
            BalanceAlgorithm::UrlHash => {
                self.modulo_hash_candidates(upstream, ctx.path, excluded_url)
            }
            BalanceAlgorithm::ConsistentHash => {
                self.consistent_hash_candidates(upstream, ctx.path, excluded_url)
            }
        };
        let selection = self.acquire_selected_target(candidates.enabled, candidates.fallback)?;
        let target = selection.target;
        if let Some(key) = sticky_key {
            let now = Instant::now();
            self.stick_table.bind(
                &upstream.name,
                key,
                &target.url,
                now + Duration::from_secs(upstream.sticky.ttl_seconds),
                now,
            );
        }

        Some(SelectedTarget {
            url: target.url.clone(),
            upstream: upstream.name.clone(),
            active_connection: TargetLease {
                _runtime_lease: selection._runtime_lease,
            },
        })
    }

    fn select_sticky<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        sticky_key: &str,
    ) -> Option<TargetSelection<'a>> {
        let now = Instant::now();
        let target_url = self.stick_table.lookup(&upstream.name, sticky_key, now)?;
        let target = upstream.targets.iter().find(|target| {
            target.url == target_url && self.target_is_selectable(target, None, false)
        })?;
        self.runtime_state
            .acquire_available_target(&target.key)
            .map(|runtime_lease| TargetSelection {
                target,
                _runtime_lease: runtime_lease,
            })
    }

    fn weighted_round_robin_candidates<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
    ) -> CandidateSet<'a> {
        let enabled = self.weighted_round_robin_candidate_order(upstream, excluded_url, true);
        let fallback = if enabled.is_empty() {
            self.weighted_round_robin_candidate_order(upstream, excluded_url, false)
        } else {
            upstream
                .targets
                .iter()
                .filter(|target| self.target_is_selectable(target, excluded_url, true))
                .collect()
        };

        CandidateSet { enabled, fallback }
    }

    fn least_connections_candidates<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
    ) -> CandidateSet<'a> {
        let mut enabled: Vec<_> = upstream
            .targets
            .iter()
            .filter(|target| self.target_is_selectable(target, excluded_url, false))
            .collect();
        enabled.sort_by_key(|target| self.runtime_state.target_active_connections(&target.key));

        let mut fallback: Vec<_> = upstream
            .targets
            .iter()
            .filter(|target| self.target_is_selectable(target, excluded_url, true))
            .collect();
        fallback.sort_by_key(|target| self.runtime_state.target_active_connections(&target.key));

        CandidateSet { enabled, fallback }
    }

    fn modulo_hash_candidates<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        key: &str,
        excluded_url: Option<&str>,
    ) -> CandidateSet<'a> {
        let enabled = rotate_candidates(
            upstream
                .targets
                .iter()
                .filter(|target| self.target_is_selectable(target, excluded_url, false))
                .collect(),
            stable_hash(&key),
        );
        let fallback = rotate_candidates(
            upstream
                .targets
                .iter()
                .filter(|target| self.target_is_selectable(target, excluded_url, true))
                .collect(),
            stable_hash(&key),
        );

        CandidateSet { enabled, fallback }
    }

    fn consistent_hash_candidates<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        key: &str,
        excluded_url: Option<&str>,
    ) -> CandidateSet<'a> {
        if upstream.hash_ring.is_empty() {
            return self.modulo_hash_candidates(upstream, key, excluded_url);
        }
        let key_hash = stable_hash(&key);
        let start = upstream
            .hash_ring
            .partition_point(|(hash, _)| *hash < key_hash);

        CandidateSet {
            enabled: self.consistent_hash_candidate_order(upstream, excluded_url, start, true),
            fallback: self.consistent_hash_candidate_order(upstream, excluded_url, start, false),
        }
    }

    fn weighted_round_robin_candidate_order<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
        require_available: bool,
    ) -> Vec<&'a WeightedTarget> {
        let mut candidates = Vec::new();
        let mut total_weight = 0u32;
        for target in upstream
            .targets
            .iter()
            .filter(|target| self.target_is_selectable(target, excluded_url, !require_available))
        {
            let Some(next_weight) = total_weight.checked_add(target.weight) else {
                break;
            };
            total_weight = next_weight;
            candidates.push((total_weight, target));
        }
        if total_weight == 0 {
            return Vec::new();
        }

        let slot = upstream.counter.fetch_add(1, Ordering::Relaxed) % total_weight;
        let start = candidates
            .iter()
            .position(|(cumulative_weight, _)| *cumulative_weight > slot)
            .unwrap_or(0);
        rotate_candidates(
            candidates.into_iter().map(|(_, target)| target).collect(),
            start as u64,
        )
    }

    fn consistent_hash_candidate_order<'a>(
        &'a self,
        upstream: &'a WeightedUpstream,
        excluded_url: Option<&str>,
        start: usize,
        require_available: bool,
    ) -> Vec<&'a WeightedTarget> {
        let mut candidates = Vec::new();
        for offset in 0..upstream.hash_ring.len() {
            let idx = (start + offset) % upstream.hash_ring.len();
            let target = &upstream.targets[upstream.hash_ring[idx].1];
            if self.target_is_selectable(target, excluded_url, !require_available)
                && !candidates
                    .iter()
                    .any(|existing: &&WeightedTarget| existing.key == target.key)
            {
                candidates.push(target);
            }
        }
        candidates
    }

    fn acquire_selected_target<'a>(
        &'a self,
        enabled: Vec<&'a WeightedTarget>,
        fallback: Vec<&'a WeightedTarget>,
    ) -> Option<TargetSelection<'a>> {
        self.acquire_selected_target_with(enabled, fallback, |key| {
            self.runtime_state.acquire_available_target(key)
        })
    }

    fn acquire_selected_target_with<'a>(
        &'a self,
        enabled: Vec<&'a WeightedTarget>,
        fallback: Vec<&'a WeightedTarget>,
        mut acquire_available: impl FnMut(&TargetKey) -> Option<crate::runtime::state::TargetLease>,
    ) -> Option<TargetSelection<'a>> {
        let enabled_keys: Vec<TargetKey> =
            enabled.iter().map(|target| target.key.clone()).collect();
        for target in enabled {
            if let Some(runtime_lease) = acquire_available(&target.key) {
                return Some(TargetSelection {
                    target,
                    _runtime_lease: runtime_lease,
                });
            }
        }

        for target in fallback {
            if let Some(runtime_lease) = self
                .runtime_state
                .acquire_unavailable_target_if_no_enabled(&target.key, &enabled_keys)
            {
                return Some(TargetSelection {
                    target,
                    _runtime_lease: runtime_lease,
                });
            }
        }

        None
    }

    #[cfg(test)]
    fn acquire_selected_target_for_test<'a>(
        &'a self,
        enabled: Vec<&'a WeightedTarget>,
        fallback: Vec<&'a WeightedTarget>,
        acquire_available: impl FnMut(&TargetKey) -> Option<crate::runtime::state::TargetLease>,
    ) -> Option<TargetSelection<'a>> {
        self.acquire_selected_target_with(enabled, fallback, acquire_available)
    }

    #[cfg(test)]
    pub(crate) fn runtime_state_for_test(&self) -> RuntimeState {
        self.runtime_state.clone()
    }

    pub(crate) fn stick_table_snapshot(&self, now: Instant) -> Vec<StickSnapshotEntry> {
        self.stick_table.snapshot(now)
    }

    #[cfg(test)]
    fn stick_table_for_test(&self) -> StickTable {
        self.stick_table.clone()
    }

    fn target_is_selectable(
        &self,
        target: &WeightedTarget,
        excluded_url: Option<&str>,
        allow_unavailable: bool,
    ) -> bool {
        self.target_is_healthy(target)
            && Some(target.url.as_str()) != excluded_url
            && (allow_unavailable || self.runtime_state.target_available(&target.key))
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

fn rotate_candidates<T>(candidates: Vec<T>, start: u64) -> Vec<T> {
    let mut candidates = candidates;
    if candidates.is_empty() {
        return candidates;
    }
    let idx = (start as usize) % candidates.len();
    candidates.rotate_left(idx);
    candidates
}

impl WeightedUpstream {
    fn new(upstream: Upstream, runtime_state: &RuntimeState) -> Self {
        let mut total_weight = 0u32;
        let mut targets = Vec::new();
        let upstream_name = upstream.name;
        let balance = upstream.balance;
        let sticky = upstream.sticky;
        let health_enabled = upstream.health_check.enabled;

        for target in upstream.targets.into_iter() {
            let key = TargetKey::new(&upstream_name, &target.url);
            let effective_weight = runtime_state.target_effective_weight(&key, target.weight);
            if effective_weight == 0 {
                continue;
            }
            let Some(next_weight) = total_weight.checked_add(effective_weight) else {
                break;
            };
            total_weight = next_weight;
            let health_key =
                health_enabled.then(|| HealthRegistry::target_key(&upstream_name, &target.url));
            targets.push(WeightedTarget {
                url: target.url,
                key,
                weight: effective_weight,
                health_key,
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
            sticky,
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
    use crate::runtime::state::{RuntimeState, TargetKey, TargetMode};
    use crate::stick::{StickyKeySource, StickyPolicy};
    use std::collections::HashMap;
    use std::time::Instant;

    fn target(url: &str, weight: u32) -> Target {
        Target {
            url: url.to_string(),
            weight,
            timeouts: Default::default(),
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
            timeouts: Default::default(),
            sticky: Default::default(),
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
        BalanceContext {
            client_ip,
            path,
            sticky_key: None,
        }
    }

    fn sticky_ctx<'a>(key: &'a str) -> BalanceContext<'a> {
        BalanceContext {
            client_ip: None,
            path: "/",
            sticky_key: Some(key),
        }
    }

    fn balancer_with(upstream: Upstream) -> Balancer {
        let mut upstreams = HashMap::new();
        upstreams.insert(upstream.name.clone(), upstream);
        Balancer::new(upstreams)
    }

    fn balancer_with_runtime(upstream: Upstream, runtime_state: RuntimeState) -> Balancer {
        let mut upstreams = HashMap::new();
        upstreams.insert(upstream.name.clone(), upstream);
        Balancer::new_with_runtime(upstreams, None, runtime_state)
    }

    fn sticky_upstream() -> Upstream {
        Upstream {
            sticky: StickyPolicy {
                enabled: true,
                source: StickyKeySource::Header {
                    name: "x-session".to_string(),
                },
                ttl_seconds: 60,
                cookie: None,
            },
            ..upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            )
        }
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
    fn sticky_key_reuses_bound_target_while_available() {
        let balancer = balancer_with(sticky_upstream());

        let first = balancer.select("backend", sticky_ctx("user-1")).unwrap();
        let first_url = first.url.clone();
        drop(first);
        let second = balancer.select("backend", sticky_ctx("user-1")).unwrap();

        assert_eq!(second.url, first_url);
        assert_eq!(
            balancer
                .stick_table_for_test()
                .snapshot(Instant::now())
                .first()
                .map(|entry| entry.target.as_str()),
            Some(first_url.as_str())
        );
    }

    #[test]
    fn sticky_key_remaps_when_bound_target_is_disabled() {
        let runtime = RuntimeState::default();
        let balancer = balancer_with_runtime(sticky_upstream(), runtime.clone());

        let first = balancer.select("backend", sticky_ctx("user-1")).unwrap();
        let first_url = first.url.clone();
        drop(first);
        runtime.set_target_mode(&TargetKey::new("backend", &first_url), TargetMode::Disabled);

        let second = balancer.select("backend", sticky_ctx("user-1")).unwrap();

        assert_ne!(second.url, first_url);
        assert_eq!(
            balancer
                .stick_table_for_test()
                .snapshot(Instant::now())
                .first()
                .map(|entry| entry.target.as_str()),
            Some(second.url.as_str())
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
    fn shared_runtime_state_preserves_active_connections_across_balancers() {
        let runtime = RuntimeState::default();
        let upstream = upstream_with_algorithm(
            "backend",
            vec![target("http://a", 1), target("http://b", 1)],
            BalanceAlgorithm::LeastConnections,
        );
        let first = balancer_with_runtime(upstream.clone(), runtime.clone());
        let selected = first.select("backend", ctx(None, "/")).unwrap();

        let second = balancer_with_runtime(upstream, runtime.clone());
        let next = second.select("backend", ctx(None, "/")).unwrap();

        assert_eq!(selected.url, "http://a");
        assert_eq!(next.url, "http://b");
        assert_eq!(
            runtime.snapshot().targets[&TargetKey::new("backend", "http://a")].active_connections,
            1
        );
    }

    #[test]
    fn runtime_drain_targets_are_skipped_when_enabled_targets_exist() {
        let runtime = RuntimeState::default();
        runtime.set_target_mode(&TargetKey::new("backend", "http://a"), TargetMode::Drain);
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            ),
            runtime,
        );

        for _ in 0..3 {
            assert_eq!(
                balancer.select("backend", ctx(None, "/")).unwrap().url,
                "http://b"
            );
        }
    }

    #[test]
    fn weighted_round_robin_rebalances_across_enabled_targets_when_one_target_is_drained() {
        let runtime = RuntimeState::default();
        runtime.set_target_mode(&TargetKey::new("backend", "http://a"), TargetMode::Drain);
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![
                    target("http://a", 1),
                    target("http://b", 1),
                    target("http://c", 1),
                ],
            ),
            runtime,
        );

        let selections: Vec<String> = (0..6)
            .map(|_| balancer.select("backend", ctx(None, "/")).unwrap().url)
            .collect();

        assert_eq!(
            selections,
            vec![
                "http://b".to_string(),
                "http://c".to_string(),
                "http://b".to_string(),
                "http://c".to_string(),
                "http://b".to_string(),
                "http://c".to_string(),
            ]
        );
    }

    #[test]
    fn runtime_disabled_targets_are_skipped_when_enabled_targets_exist() {
        let runtime = RuntimeState::default();
        runtime.set_target_mode(&TargetKey::new("backend", "http://a"), TargetMode::Disabled);
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            ),
            runtime,
        );

        for _ in 0..3 {
            assert_eq!(
                balancer.select("backend", ctx(None, "/")).unwrap().url,
                "http://b"
            );
        }
    }

    #[test]
    fn selection_falls_back_to_unavailable_targets_when_none_are_enabled() {
        let runtime = RuntimeState::default();
        runtime.set_target_mode(&TargetKey::new("backend", "http://a"), TargetMode::Drain);
        runtime.set_target_mode(&TargetKey::new("backend", "http://b"), TargetMode::Disabled);
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            ),
            runtime,
        );

        let selected = balancer.select("backend", ctx(None, "/")).unwrap();
        assert!(matches!(selected.url.as_str(), "http://a" | "http://b"));
        let key = TargetKey::new("backend", selected.url.clone());
        assert_eq!(
            balancer.runtime_state.snapshot().targets[&key].active_connections,
            1
        );
        drop(selected);
        assert_eq!(
            balancer.runtime_state.snapshot().targets[&key].active_connections,
            0
        );
    }

    #[test]
    fn acquisition_continues_to_next_enabled_candidate_after_first_candidate_fails() {
        let runtime = RuntimeState::default();
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            ),
            runtime.clone(),
        );
        let upstream = balancer.upstreams.get("backend").unwrap();
        let candidates: Vec<_> = upstream.targets.iter().collect();
        let mut attempts = 0;

        let acquired = balancer
            .acquire_selected_target_for_test(candidates, Vec::new(), |key| {
                attempts += 1;
                if attempts == 1 {
                    None
                } else {
                    runtime.acquire_available_target(key)
                }
            })
            .unwrap();

        assert_eq!(acquired.target.url, "http://b");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn acquisition_does_not_fallback_to_unavailable_target_while_enabled_candidate_remains() {
        let runtime = RuntimeState::default();
        let balancer = balancer_with_runtime(
            upstream(
                "backend",
                vec![target("http://a", 1), target("http://b", 1)],
            ),
            runtime.clone(),
        );
        let upstream = balancer.upstreams.get("backend").unwrap();
        let first = &upstream.targets[0];
        let second = &upstream.targets[1];
        let mut attempts = 0;

        let acquired =
            balancer.acquire_selected_target_for_test(vec![first, second], vec![first], |key| {
                attempts += 1;
                if key.url == "http://a" {
                    runtime.set_target_mode(key, TargetMode::Disabled);
                }
                None
            });

        assert!(acquired.is_none());
        assert_eq!(attempts, 2);
        assert_eq!(
            runtime
                .snapshot()
                .targets
                .get(&TargetKey::new("backend", "http://a"))
                .map_or(0, |target| target.active_connections),
            0
        );
        assert!(runtime.target_available(&TargetKey::new("backend", "http://b")));
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
            timeouts: Default::default(),
            sticky: Default::default(),
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
