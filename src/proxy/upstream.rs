use crate::models::{Target, Upstream};

/// Return selectable targets for an upstream, excluding zero-weight targets.
pub fn selectable_targets(upstream: &Upstream) -> Vec<&Target> {
    upstream
        .targets
        .iter()
        .filter(|target| target.weight > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::selectable_targets;
    use crate::models::{Target, Upstream};

    #[test]
    fn skips_zero_weight_targets() {
        let upstream = Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://a".to_string(),
                    weight: 10,
                },
                Target {
                    url: "http://b".to_string(),
                    weight: 0,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        let targets = selectable_targets(&upstream);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "http://a");
    }
}
