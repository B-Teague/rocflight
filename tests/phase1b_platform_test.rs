//! Phase 1B Platform Loading Tests
//!
//! Tests the platform infrastructure:
//! - Platform loader creation
//! - Mock platform loading
//! - Caching mechanism
//! - Module and export lookup

#[cfg(test)]
mod phase1b_platform_tests {
    use rocflight::platform::{PlatformLoader, PlatformModule, ModuleExport};
    use rocflight::platform::cache::{clear_cache, get_platform};

    #[test]
    fn test_platform_loader_creation() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://github.com/roc-lang/basic-cli/releases/download/0.20.0/X73hGh05nNTkDHU06FHC0YfFaQB1pimX7gncRcao5mU.tar.br".to_string(),
        );

        assert_eq!(loader.name, "pf");
        assert!(loader.url.contains("basic-cli"));
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
        assert_eq!(platform.url, "https://example.com/platform.tar.br");

        // Mock platform should have Stdout module
        assert!(platform.get_module("Stdout").is_some());
    }

    #[test]
    fn test_stdout_module_has_line() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        let platform = loader.load().expect("Failed to load platform");
        let stdout = platform.get_module("Stdout").expect("Stdout not found");

        // The desugarer removes !, so we look for "line" not "line!"
        let line_export = stdout.get_export("line").expect("line export not found");

        match line_export {
            ModuleExport::Function { name, type_sig } => {
                assert_eq!(name, "line");
                assert!(type_sig.contains("Str"));
                assert!(type_sig.contains("Result"));
            }
            _ => panic!("Expected function export"),
        }
    }

    #[test]
    fn test_platform_module_names() {
        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        let platform = loader.load().expect("Failed to load platform");
        let names = platform.module_names();

        assert!(names.contains(&"Stdout"));
    }

    #[test]
    fn test_platform_caching() {
        clear_cache();

        let loader = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform.tar.br".to_string(),
        );

        // First load
        let platform1 = loader.load_cached().expect("First load failed");

        // Second load should hit cache
        let platform2 = loader.load_cached().expect("Second load failed");

        assert_eq!(platform1.name, platform2.name);
        assert_eq!(platform1.url, platform2.url);

        // Verify cache has the platform
        let cached = get_platform(&loader.url).expect("Platform not in cache");
        assert_eq!(cached.name, "pf");

        clear_cache();
    }

    #[test]
    fn test_platform_ref_creation() {
        let pf_ref = rocflight::PlatformRef {
            name: "pf".to_string(),
            url: "https://example.com/platform.tar.br".to_string(),
        };

        assert_eq!(pf_ref.name, "pf");
        assert!(pf_ref.url.contains("tar.br"));
    }

    #[test]
    fn test_multiple_platforms_in_cache() {
        clear_cache();

        let loader1 = PlatformLoader::new(
            "pf".to_string(),
            "https://example.com/platform1.tar.br".to_string(),
        );

        let loader2 = PlatformLoader::new(
            "pf2".to_string(),
            "https://example.com/platform2.tar.br".to_string(),
        );

        let _p1 = loader1.load_cached().expect("First platform failed");
        let _p2 = loader2.load_cached().expect("Second platform failed");

        // Both should be in cache with different URLs
        assert!(get_platform("https://example.com/platform1.tar.br").is_some());
        assert!(get_platform("https://example.com/platform2.tar.br").is_some());

        clear_cache();
    }

    #[test]
    fn test_platform_module_export_lookup() {
        let mut module = PlatformModule::new("Stdout".to_string());

        // Add line function
        module.add_export(
            "line".to_string(),
            ModuleExport::function_export(
                "line".to_string(),
                "Str -> Result {} [IOErr]".to_string(),
            ),
        );

        // Add write function
        module.add_export(
            "write".to_string(),
            ModuleExport::function_export(
                "write".to_string(),
                "Str -> Result {} [IOErr]".to_string(),
            ),
        );

        // Lookup should work
        assert!(module.get_export("line").is_some());
        assert!(module.get_export("write").is_some());
        assert!(module.get_export("missing").is_none());

        let names = module.export_names();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_module_export_name() {
        let export = ModuleExport::function_export(
            "line".to_string(),
            "Str -> Result {} [IOErr]".to_string(),
        );

        assert_eq!(export.name(), "line");
    }
}
