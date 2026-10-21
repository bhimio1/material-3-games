//! Small HTTP helpers for Steam endpoints, with an on-disk cache.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const UA: &str = concat!("m3-games/", env!("CARGO_PKG_VERSION"));

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent(UA).build()
}

/// GET a URL as text.
pub fn get(url: &str) -> Result<String, String> {
    get_with_cookie(url, "")
}

/// GET with a `Cookie` header (empty = none).
pub fn get_with_cookie(url: &str, cookie: &str) -> Result<String, String> {
    let mut req = agent().get(url).set("Accept-Language", "en-US,en;q=0.8");
    if !cookie.is_empty() {
        req = req.set("Cookie", cookie);
    }
    req.call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
}

pub fn cache_file(kind: &str, key: &str) -> PathBuf {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    crate::library::cache_dir().join(kind).join(safe)
}

/// GET with a cache: fresh copies younger than `max_age` are used as-is;
/// when the network fails, any cached copy is used.
pub fn get_cached(kind: &str, key: &str, url: &str, max_age: Duration) -> Result<String, String> {
    get_cached_with_cookie(kind, key, url, "", max_age)
}

/// [`get_cached`] with a `Cookie` header.
pub fn get_cached_with_cookie(kind: &str, key: &str, url: &str, cookie: &str, max_age: Duration) -> Result<String, String> {
    let file = cache_file(kind, key);
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < max_age);
    if fresh {
        if let Ok(text) = std::fs::read_to_string(&file) {
            return Ok(text);
        }
    }
    match get_with_cookie(url, cookie) {
        Ok(text) => {
            let _ = crate::library::write_atomic(&file, &text);
            Ok(text)
        }
        Err(e) => std::fs::read_to_string(&file).map_err(|_| e),
    }
}

/// Download a binary file into the cache (images), returning its path.
pub fn fetch_file(kind: &str, url: &str) -> Result<PathBuf, String> {
    let file = cache_file(kind, url);
    if file.is_file() {
        return Ok(file);
    }
    let resp = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader().take(20 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file)
}
