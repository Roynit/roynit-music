//! Short-lived, per-engine search results. Never cache errors or cross a session/network change.
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct SearchCache(Mutex<State>);
#[derive(Default)]
struct State {
    revision: u64,
    entries: HashMap<String, (Instant, Value)>,
}
impl SearchCache {
    pub fn clear(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.revision = state.revision.wrapping_add(1);
            state.entries.clear();
        }
    }
    pub fn run(&self, key: String, fetch: impl FnOnce() -> Result<Value>) -> Result<Value> {
        crate::jobs::check_cancelled()?;
        let revision = {
            let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((time, value)) = state.entries.get(&key)
                && time.elapsed() < Duration::from_secs(120)
            {
                return Ok(value.clone());
            }
            state.revision
        };
        let value = fetch()?;
        crate::jobs::check_cancelled()?;
        if let Ok(mut state) = self.0.lock()
            && state.revision == revision
        {
            state
                .entries
                .retain(|_, (time, _)| time.elapsed() < Duration::from_secs(120));
            if state.entries.len() >= 64
                && let Some(old) = state
                    .entries
                    .iter()
                    .min_by_key(|(_, v)| v.0)
                    .map(|(k, _)| k.clone())
            {
                state.entries.remove(&old);
            }
            state.entries.insert(key, (Instant::now(), value.clone()));
        }
        Ok(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reuses_success_but_not_errors_or_results_from_old_session() {
        let cache = SearchCache::default();
        assert!(
            cache
                .run("q".into(), || anyhow::bail!("network failure"))
                .is_err()
        );
        assert_eq!(
            cache.run("q".into(), || Ok(serde_json::json!(1))).unwrap(),
            1
        );
        assert_eq!(
            cache.run("q".into(), || panic!("must use cache")).unwrap(),
            1
        );
        cache.clear();
        cache
            .run("q".into(), || {
                cache.clear();
                Ok(serde_json::json!(2))
            })
            .unwrap();
        assert_eq!(
            cache.run("q".into(), || Ok(serde_json::json!(3))).unwrap(),
            3
        );
    }
}
