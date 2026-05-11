use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_serde() {
        let target = Target {
            url: "http://localhost:8080".to_string(),
            weight: 100,
        };
        let json = serde_json::to_string(&target).unwrap();
        let parsed: Target = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "http://localhost:8080");
        assert_eq!(parsed.weight, 100);
    }

    #[test]
    fn test_target_clone_and_partial_eq() {
        let target1 = Target {
            url: "http://localhost:8080".to_string(),
            weight: 50,
        };
        let target2 = target1.clone();
        assert_eq!(target1, target2);
    }

    #[test]
    fn test_upstream_serde() {
        let upstream = Upstream {
            name: "backend".to_string(),
            targets: vec![
                Target {
                    url: "http://localhost:8080".to_string(),
                    weight: 80,
                },
                Target {
                    url: "http://localhost:8081".to_string(),
                    weight: 20,
                },
            ],
        };
        let json = serde_json::to_string(&upstream).unwrap();
        let parsed: Upstream = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "backend");
        assert_eq!(parsed.targets.len(), 2);
        assert_eq!(parsed.targets[0].url, "http://localhost:8080");
        assert_eq!(parsed.targets[0].weight, 80);
        assert_eq!(parsed.targets[1].url, "http://localhost:8081");
        assert_eq!(parsed.targets[1].weight, 20);
    }

    #[test]
    fn test_upstream_single_target() {
        let upstream = Upstream {
            name: "single".to_string(),
            targets: vec![Target {
                url: "http://localhost:9090".to_string(),
                weight: 100,
            }],
        };
        assert_eq!(upstream.targets.len(), 1);
        assert_eq!(upstream.targets[0].url, "http://localhost:9090");
    }

    #[test]
    fn test_upstream_clone() {
        let upstream = Upstream {
            name: "clone-test".to_string(),
            targets: vec![
                Target {
                    url: "http://localhost:8080".to_string(),
                    weight: 50,
                },
                Target {
                    url: "http://localhost:8081".to_string(),
                    weight: 50,
                },
            ],
        };
        let cloned = upstream.clone();
        assert_eq!(upstream, cloned);
        assert_eq!(cloned.name, "clone-test");
        assert_eq!(cloned.targets.len(), 2);
    }

    #[test]
    fn test_target_debug() {
        let target = Target {
            url: "http://debug:9999".to_string(),
            weight: 42,
        };
        let debug_str = format!("{:?}", target);
        assert!(debug_str.contains("http://debug:9999"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_upstream_debug() {
        let upstream = Upstream {
            name: "debug-upstream".to_string(),
            targets: vec![],
        };
        let debug_str = format!("{:?}", upstream);
        assert!(debug_str.contains("debug-upstream"));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub url: String,
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Upstream {
    pub name: String,
    pub targets: Vec<Target>,
}
