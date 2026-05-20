use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TargetKey {
    pub upstream: String,
    pub url: String,
}

impl TargetKey {
    pub fn new(upstream: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            upstream: upstream.into(),
            url: url.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetMode {
    Enabled,
    Disabled,
    Drain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetRuntime {
    pub mode: TargetMode,
    pub active_connections: u32,
    pub weight_override: Option<u32>,
    pub last_error: Option<String>,
}

impl Default for TargetRuntime {
    fn default() -> Self {
        Self {
            mode: TargetMode::Enabled,
            active_connections: 0,
            weight_override: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeState {
    targets: Arc<RwLock<HashMap<TargetKey, TargetRuntime>>>,
}

#[derive(Debug)]
pub struct TargetLease {
    state: RuntimeState,
    key: TargetKey,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeSnapshot {
    pub targets: HashMap<TargetKey, TargetRuntime>,
}

impl RuntimeState {
    pub fn target_available(&self, key: &TargetKey) -> bool {
        self.targets
            .read()
            .expect("runtime target state lock poisoned")
            .get(key)
            .is_none_or(|target| target.mode == TargetMode::Enabled)
    }

    pub fn acquire_target(&self, key: &TargetKey) -> Option<TargetLease> {
        self.acquire_target_with_mode(key, false)
    }

    pub fn acquire_available_target(&self, key: &TargetKey) -> Option<TargetLease> {
        self.acquire_target_with_mode(key, true)
    }

    pub fn acquire_unavailable_target_if_no_enabled(
        &self,
        key: &TargetKey,
        enabled_keys: &[TargetKey],
    ) -> Option<TargetLease> {
        let mut targets = self
            .targets
            .write()
            .expect("runtime target state lock poisoned");
        if enabled_keys.iter().any(|enabled_key| {
            targets
                .get(enabled_key)
                .is_none_or(|target| target.mode == TargetMode::Enabled)
        }) {
            return None;
        }

        let target = targets.entry(key.clone()).or_default();
        target.active_connections = target.active_connections.saturating_add(1);
        Some(TargetLease {
            state: self.clone(),
            key: key.clone(),
        })
    }

    fn acquire_target_with_mode(
        &self,
        key: &TargetKey,
        require_available: bool,
    ) -> Option<TargetLease> {
        let mut targets = self
            .targets
            .write()
            .expect("runtime target state lock poisoned");
        let target = targets.entry(key.clone()).or_default();
        if require_available && target.mode != TargetMode::Enabled {
            return None;
        }
        target.active_connections = target.active_connections.saturating_add(1);
        Some(TargetLease {
            state: self.clone(),
            key: key.clone(),
        })
    }

    pub fn set_target_mode(&self, key: &TargetKey, mode: TargetMode) {
        self.targets
            .write()
            .expect("runtime target state lock poisoned")
            .entry(key.clone())
            .or_default()
            .mode = mode;
    }

    pub fn set_target_weight(&self, key: &TargetKey, weight: Option<u32>) {
        self.targets
            .write()
            .expect("runtime target state lock poisoned")
            .entry(key.clone())
            .or_default()
            .weight_override = weight;
    }

    pub fn target_effective_weight(&self, key: &TargetKey, configured_weight: u32) -> u32 {
        self.targets
            .read()
            .expect("runtime target state lock poisoned")
            .get(key)
            .and_then(|target| target.weight_override)
            .unwrap_or(configured_weight)
    }

    pub fn target_active_connections(&self, key: &TargetKey) -> u32 {
        self.targets
            .read()
            .expect("runtime target state lock poisoned")
            .get(key)
            .map_or(0, |target| target.active_connections)
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            targets: self
                .targets
                .read()
                .expect("runtime target state lock poisoned")
                .clone(),
        }
    }
}

impl Drop for TargetLease {
    fn drop(&mut self) {
        let mut targets = self
            .state
            .targets
            .write()
            .expect("runtime target state lock poisoned");
        if let Some(target) = targets.get_mut(&self.key) {
            target.active_connections = target.active_connections.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeState, TargetKey, TargetMode};

    #[test]
    fn runtime_state_tracks_target_status_and_connections() {
        let state = RuntimeState::default();
        let key = TargetKey::new("api", "http://127.0.0.1:8080");

        assert!(state.target_available(&key));
        let lease = state.acquire_target(&key).unwrap();
        assert_eq!(state.snapshot().targets[&key].active_connections, 1);
        drop(lease);
        assert_eq!(state.snapshot().targets[&key].active_connections, 0);

        state.set_target_mode(&key, TargetMode::Drain);
        assert!(!state.target_available(&key));
        state.set_target_mode(&key, TargetMode::Enabled);
        state.set_target_weight(&key, Some(25));
        assert_eq!(state.target_effective_weight(&key, 100), 25);
    }

    #[test]
    fn acquire_available_target_rejects_unavailable_targets_without_incrementing() {
        let state = RuntimeState::default();
        let drain = TargetKey::new("api", "http://127.0.0.1:8080");
        let disabled = TargetKey::new("api", "http://127.0.0.1:8081");

        state.set_target_mode(&drain, TargetMode::Drain);
        state.set_target_mode(&disabled, TargetMode::Disabled);

        assert!(state.acquire_available_target(&drain).is_none());
        assert!(state.acquire_available_target(&disabled).is_none());
        assert_eq!(state.snapshot().targets[&drain].active_connections, 0);
        assert_eq!(state.snapshot().targets[&disabled].active_connections, 0);
    }

    #[test]
    fn acquire_unavailable_target_requires_all_enabled_candidates_to_be_unavailable() {
        let state = RuntimeState::default();
        let fallback = TargetKey::new("api", "http://127.0.0.1:8080");
        let still_enabled = TargetKey::new("api", "http://127.0.0.1:8081");

        state.set_target_mode(&fallback, TargetMode::Drain);
        assert!(state
            .acquire_unavailable_target_if_no_enabled(
                &fallback,
                std::slice::from_ref(&still_enabled),
            )
            .is_none());
        assert_eq!(state.snapshot().targets[&fallback].active_connections, 0);

        state.set_target_mode(&still_enabled, TargetMode::Disabled);
        let lease = state
            .acquire_unavailable_target_if_no_enabled(&fallback, &[still_enabled])
            .unwrap();
        assert_eq!(state.snapshot().targets[&fallback].active_connections, 1);
        drop(lease);
        assert_eq!(state.snapshot().targets[&fallback].active_connections, 0);
    }
}
