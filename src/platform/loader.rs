//! Mock platform, kept for the import-resolution tests that predate real loading.
//!
//! REAL platform loading lives in `platform::resolve` (URL → cached sources) and
//! `platform::real` (reading a platform's own `.roc` files). Nothing here downloads or
//! decompresses anything: `roc` does that, and the interpreter reads what it extracted.
//!
//! This module's header used to promise HTTP download, brotli decompression and tar
//! extraction. None of it was implemented, and none of it is needed — the archive
//! format is `.tar.zst` now anyway. Kept only so the existing tests keep exercising
//! module/export lookup against a fixture.

use super::{Platform, PlatformModule, ModuleExport};
use std::collections::HashMap;

/// Platform loader - downloads and parses platforms
pub struct PlatformLoader {
    /// URL to fetch platform from
    pub url: String,
    /// Platform name (e.g., "pf")
    pub name: String,
}

impl PlatformLoader {
    /// Create new platform loader
    pub fn new(name: String, url: String) -> Self {
        PlatformLoader { url, name }
    }

    /// Load platform (download, decompress, parse)
    ///
    /// For Phase 1B, this is a placeholder that creates a mock platform
    /// with basic Stdout.line! function. Full implementation will follow.
    pub fn load(&self) -> Result<Platform, String> {
        // Phase 1B: Create mock platform with basic functions
        // Real implementation (Phase 1B full) will:
        // 1. Download from self.url
        // 2. Decompress brotli
        // 3. Extract tar
        // 4. Parse .roc files
        // 5. Build module list

        eprintln!("[Platform] Loading platform: {} from {}", self.name, self.url);

        // For now, create a mock platform with Stdout module
        let mut modules = HashMap::new();

        // Create Stdout module. Exports keep their real names: the `!` is part of
        // the identifier, so the export is `line!`, not `line`.
        let mut stdout_module = PlatformModule::new("Stdout".to_string());
        for name in ["line!", "write!"] {
            stdout_module.add_export(
                name.to_string(),
                ModuleExport::function_export(
                    name.to_string(),
                    // Matches roc-compiler/test/fx/platform/Stdout.roc. `!` means
                    // effectful (`=>`), not "returns a Result": the platform decides
                    // the return type, and this one returns `{}`.
                    "Str => {}".to_string(),
                ),
            );
        }
        modules.insert("Stdout".to_string(), stdout_module);

        eprintln!("[Platform] Mock platform loaded with Stdout module");

        Ok(Platform {
            name: self.name.clone(),
            url: self.url.clone(),
            modules,
        })
    }

    /// Load platform with caching
    pub fn load_cached(&self) -> Result<Platform, String> {
        use super::cache::{get_platform, cache_platform};

        // Check if already cached
        if let Some(platform) = get_platform(&self.url) {
            eprintln!("[Platform] Using cached platform: {}", self.name);
            return Ok(platform);
        }

        // Load fresh
        let platform = self.load()?;

        // Cache for future use
        cache_platform(self.url.clone(), platform.clone());

        Ok(platform)
    }
}

// Placeholder for future HTTP download implementation
// These will be implemented in Phase 1B full when adding reqwest and brotli dependencies
#[allow(dead_code)]
mod download {
    use super::*;

    /// Download platform file from URL
    ///
    /// Returns brotli-compressed tar archive bytes
    async fn _download(_url: &str) -> Result<Vec<u8>, String> {
        // Will use reqwest crate when implemented
        todo!("HTTP download not yet implemented")
    }

    /// Decompress brotli archive
    fn _decompress_brotli(_compressed: &[u8]) -> Result<Vec<u8>, String> {
        // Will use brotli crate when implemented
        todo!("Brotli decompression not yet implemented")
    }

    /// Extract tar archive
    fn _extract_tar(_tar_bytes: &[u8]) -> Result<HashMap<String, String>, String> {
        // Returns HashMap<filename, content>
        // Will use tar crate when implemented
        todo!("Tar extraction not yet implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_loader_creation() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );
        assert_eq!(loader.name, "pf");
        assert!(loader.url.contains("tar.br"));
    }

    #[test]
    fn test_load_mock_platform() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        let platform = loader.load().expect("Failed to load platform");
        assert_eq!(platform.name, "pf");
        assert!(platform.get_module("Stdout").is_some());
    }

    #[test]
    fn test_platform_has_line_export() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        let platform = loader.load().expect("Failed to load platform");
        let stdout = platform.get_module("Stdout").expect("Stdout module not found");
        let line_export = stdout.get_export("line!").expect("line! export not found");

        match line_export {
            ModuleExport::Function { name, type_sig } => {
                assert_eq!(name, "line!");
                assert!(type_sig.contains("Str"));
            }
            _ => panic!("Expected function export"),
        }
    }

    #[test]
    fn test_caching() {
        use super::super::cache::{clear_cache, lock_for_test};

        let _guard = lock_for_test();
        clear_cache();

        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        // Load twice - second should hit cache
        let platform1 = loader.load_cached().expect("First load failed");
        let platform2 = loader.load_cached().expect("Second load failed");

        assert_eq!(platform1.name, platform2.name);
        assert_eq!(platform1.url, platform2.url);
    }
}
