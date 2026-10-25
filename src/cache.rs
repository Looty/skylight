//! Tiny on-disk cache of raw API responses, keyed by request.
//! Fresh entries skip the network; stale ones are still used when offline.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    t: u64,
    v: Value,
}

const KEEP_SECS: u64 = 24 * 3600;

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(|| {
    let mut m: HashMap<String, Entry> =
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let now = now();
    m.retain(|_, e| now.saturating_sub(e.t) < KEEP_SECS);
    Mutex::new(m)
});

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Skylight").join("cache.json"))
}

/// `max_age`: `Some(secs)` returns only fresh entries, `None` returns whatever is stored.
/// The second value is the entry's age in seconds.
pub fn get(key: &str, max_age: Option<u64>) -> Option<(Value, u64)> {
    let c = CACHE.lock().ok()?;
    let e = c.get(key)?;
    let age = now().saturating_sub(e.t);
    match max_age {
        Some(m) if age >= m => None,
        _ => Some((e.v.clone(), age)),
    }
}

pub fn put(key: &str, v: Value) {
    let Ok(mut c) = CACHE.lock() else { return };
    c.insert(key.to_string(), Entry { t: now(), v });
    if let Some(p) = path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string(&*c) {
            let _ = std::fs::write(p, t);
        }
    }
}
