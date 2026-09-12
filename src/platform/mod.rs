//! Platform System - Download, parse, and cache Roc platforms
//!
//! Platforms provide access to system I/O and other services.
//! Example: https://github.com/roc-lang/basic-cli/releases/download/0.20.0/...tar.br
//!
//! Architecture:
//! 1. Download and cache platform files
//! 2. Decompress brotli archives
//! 3. Extract tar files
//! 4. Parse .roc modules from platform
//! 5. Extract function signatures and types
//! 6. Store in global cache for import resolution

pub mod loader;
pub mod cache;
pub mod module;

pub use loader::PlatformLoader;
pub use cache::PlatformCache;
pub use module::{PlatformModule, ModuleExport};

use std::collections::HashMap;

/// Reference to a platform in an app declaration
/// Example: `pf: platform "https://github.com/roc-lang/basic-cli/.../tar.br"`
#[derive(Debug, Clone)]
pub struct PlatformRef {
    /// Platform name as bound in app declaration (e.g., "pf")
    pub name: String,
    /// URL to download platform from
    pub url: String,
}

/// Platform loaded and cached in memory
#[derive(Debug, Clone)]
pub struct Platform {
    /// Platform name (e.g., "pf")
    pub name: String,
    /// URL it was loaded from
    pub url: String,
    /// All modules in the platform
    pub modules: HashMap<String, PlatformModule>,
}

impl Platform {
    /// Look up a module by name (e.g., "Stdout")
    pub fn get_module(&self, name: &str) -> Option<&PlatformModule> {
        self.modules.get(name)
    }

    /// Get all available module names
    pub fn module_names(&self) -> Vec<&str> {
        self.modules.keys().map(|k| k.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_ref_creation() {
        let platform = PlatformRef {
            name: "pf".to_string(),
            url: "https://example.com/platform.tar.br".to_string(),
        };
        assert_eq!(platform.name, "pf");
        assert!(platform.url.contains("tar.br"));
    }

    #[test]
    fn test_platform_lookup() {
        let mut modules = HashMap::new();
        let module = PlatformModule {
            name: "Stdout".to_string(),
            exports: HashMap::new(),
        };
        modules.insert("Stdout".to_string(), module);

        let platform = Platform {
            name: "pf".to_string(),
            url: "https://example.com/platform.tar.br".to_string(),
            modules,
        };

        assert!(platform.get_module("Stdout").is_some());
        assert!(platform.get_module("Stdin").is_none());
    }
}
