use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::models::Upstream;
use crate::proxy::upstream::selectable_targets;

pub struct Balancer {
    upstreams: HashMap<String, Upstream>,
    counters: HashMap<String, AtomicU32>,
}

impl Balancer {
    pub fn new(upstreams: HashMap<String, Upstream>) -> Self {
        let counters = upstreams
            .keys()
            .map(|name| (name.clone(), AtomicU32::new(0)))
            .collect();

        Self {
            upstreams,
            counters,
        }
    }

    pub fn select(&self, upstream_name: &str) -> Option<String> {
        let upstream = self.upstreams.get(upstream_name)?;
        let targets = selectable_targets(upstream);
        if targets.is_empty() {
            return None;
        }

        let total_weight = targets
            .iter()
            .try_fold(0u32, |sum, target| sum.checked_add(target.weight))?;
        if total_weight == 0 {
            return None;
        }

        let counter = self.counters.get(upstream_name)?;
        let slot = counter.fetch_add(1, Ordering::Relaxed) % total_weight;
        let mut cumulative_weight = 0u32;

        targets.into_iter().find_map(|target| {
            cumulative_weight += target.weight;
            if slot < cumulative_weight {
                Some(target.url.clone())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Balancer;
    use crate::models::{Target, Upstream};
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
            targets,
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
}
