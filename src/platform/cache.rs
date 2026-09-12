//! Global platform cache - singleton pattern for in-memory platform storage

use super::Platform;
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// Global platform cache - stores all loaded platforms in memory
pub static PLATFORM_CACHE: Lazy<Mutex<PlatformCache>> =
    Lazy::new(|| Mutex::new(PlatformCache::new()));

/// Cache for loaded platforms - indexed by URL
pub struct PlatformCache {
    /// Map from platform URL to loaded Platform
    platforms: HashMap<String, Platform>,
}

impl PlatformCache {
    /// Create new empty cache
    pub fn new() -> Self {
        PlatformCache {
            platforms: HashMap::new(),
        }
    }

    /// Check if platform is cached
    pub fn contains(&self, url: &str) -> bool {
        self.platforms.contains_key(url)
    }

    /// Get cached platform
    pub fn get(&self, url: &str) -> Option<Platform> {
        self.platforms.get(url).cloned()
    }

    /// Store platform in cache
    pub fn insert(&mut self, url: String, platform: Platform) {
        self.platforms.insert(url, platform);
    }

    /// Get all cached platform URLs
    pub fn cached_urls(&self) -> Vec<String> {
        self.platforms.keys().cloned().collect()
    }

    /// Number of cached platforms
    pub fn len(&self) -> usize {
        self.platforms.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.platforms.is_empty()
    }

    /// Clear all cached platforms
    pub fn clear(&mut self) {
        self.platforms.clear();
    }
}

impl Default for PlatformCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Get or load a platform from cache
///
/// If the platform is already cached, returns it.
/// Otherwise, returns None (caller must load it).
pub fn get_platform(url: &str) -> Option<Platform> {
    let cache = PLATFORM_CACHE.lock().unwrap();
    cache.get(url)
}

/// Store a platform in the global cache
pub fn cache_platform(url: String, platform: Platform) {
    let mut cache = PLATFORM_CACHE.lock().unwrap();
    cache.insert(url, platform);
}

/// Clear the global platform cache
pub fn clear_cache() {
    let mut cache = PLATFORM_CACHE.lock().unwrap();
    cache.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_platform() -> Platform {
        Platform {
            name: "test".to_string(),
            url: "https://example.com/test.tar.br".to_string(),
            modules: HashMap::new(),
        }
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = PlatformCache::new();
        let platform = create_test_platform();

        cache.insert("https://example.com/test.tar.br".to_string(), platform.clone());
        assert!(cache.contains("https://example.com/test.tar.br"));
        assert_eq!(cache.get("https://example.com/test.tar.br").unwrap().name, "test");
    }

    #[test]
    fn test_cache_miss() {
        let cache = PlatformCache::new();
        assert!(!cache.contains("https://example.com/missing.tar.br"));
        assert!(cache.get("https://example.com/missing.tar.br").is_none());
    }

    #[test]
    fn test_cache_size() {
        let mut cache = PlatformCache::new();
        assert_eq!(cache.len(), 0);

        cache.insert("url1".to_string(), create_test_platform());
        assert_eq!(cache.len(), 1);

        cache.insert("url2".to_string(), create_test_platform());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = PlatformCache::new();
        cache.insert("url1".to_string(), create_test_platform());
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_global_cache() {
        clear_cache();

        let platform = create_test_platform();
        let url = "https://example.com/global-test.tar.br".to_string();
        cache_platform(url.clone(), platform);

        assert!(get_platform(&url).is_some());

        clear_cache();
        assert!(get_platform(&url).is_none());
    }
}
