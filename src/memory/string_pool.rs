//! Global string interning pool
//!
//! All strings are stored as &'static str for zero-copy sharing

use std::collections::HashSet;
use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Thread-safe string pool
pub struct StringPool {
    strings: HashSet<&'static str>,
}

impl StringPool {
    /// Create new string pool
    pub fn new() -> Self {
        StringPool {
            strings: HashSet::new(),
        }
    }

    /// Intern a string (get &'static str for this string)
    pub fn intern(&mut self, s: &str) -> &'static str {
        // Check if already interned
        for &interned in &self.strings {
            if interned == s {
                return interned;
            }
        }
        // New string: leak it to get &'static str
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        self.strings.insert(leaked);
        leaked
    }

    /// Get or insert (simpler interface)
    pub fn get_or_insert(&mut self, s: &str) -> &'static str {
        self.intern(s)
    }
}

impl Default for StringPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Global string pool (lazy initialized)
static GLOBAL_POOL: Lazy<Mutex<StringPool>> = Lazy::new(|| {
    Mutex::new(StringPool::new())
});

/// Intern a string globally
pub fn intern(s: &str) -> &'static str {
    let mut pool = GLOBAL_POOL.lock().expect("String pool lock poisoned");
    pool.intern(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_string() {
        let s1 = intern("hello");
        let s2 = intern("hello");
        assert_eq!(s1 as *const _, s2 as *const _); // Same pointer
    }

    #[test]
    fn test_intern_different_strings() {
        let s1 = intern("hello");
        let s2 = intern("world");
        assert_ne!(s1 as *const _, s2 as *const _); // Different pointers
    }
}
