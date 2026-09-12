//! Platform module definitions and exports

use std::collections::HashMap;

/// A module within a platform (e.g., Stdout, Stdin, File)
#[derive(Debug, Clone)]
pub struct PlatformModule {
    /// Module name (e.g., "Stdout", "Stdin")
    pub name: String,
    /// All exported items from this module
    pub exports: HashMap<String, ModuleExport>,
}

impl PlatformModule {
    /// Create new module
    pub fn new(name: String) -> Self {
        PlatformModule {
            name,
            exports: HashMap::new(),
        }
    }

    /// Add an export to this module
    pub fn add_export(&mut self, name: String, export: ModuleExport) {
        self.exports.insert(name, export);
    }

    /// Get an export by name (e.g., "line!")
    pub fn get_export(&self, name: &str) -> Option<&ModuleExport> {
        self.exports.get(name)
    }

    /// List all exported names
    pub fn export_names(&self) -> Vec<&str> {
        self.exports.keys().map(|k| k.as_str()).collect()
    }
}

/// An exported item from a platform module
#[derive(Debug, Clone)]
pub enum ModuleExport {
    /// Type definition (e.g., IOErr : [NotFound, PermissionDenied, ...])
    Type {
        name: String,
        definition: String, // For Phase 1B, store as string; will parse fully in Phase 9
    },
    /// Function definition (e.g., line! : Str => Result {} [StdoutErr IOErr])
    Function {
        name: String,
        type_sig: String, // Desugared type signature (! already removed)
    },
}

impl ModuleExport {
    /// Create a type export
    pub fn type_export(name: String, definition: String) -> Self {
        ModuleExport::Type { name, definition }
    }

    /// Create a function export
    pub fn function_export(name: String, type_sig: String) -> Self {
        ModuleExport::Function {
            name,
            type_sig,
        }
    }

    /// Get the name of this export
    pub fn name(&self) -> &str {
        match self {
            ModuleExport::Type { name, .. } => name,
            ModuleExport::Function { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_module() {
        let module = PlatformModule::new("Stdout".to_string());
        assert_eq!(module.name, "Stdout");
        assert_eq!(module.exports.len(), 0);
    }

    #[test]
    fn test_add_export() {
        let mut module = PlatformModule::new("Stdout".to_string());
        module.add_export(
            "line".to_string(),
            ModuleExport::function_export(
                "line".to_string(),
                "Str -> Result {} [StdoutErr IOErr]".to_string(),
            ),
        );

        assert_eq!(module.exports.len(), 1);
        assert!(module.get_export("line").is_some());
    }

    #[test]
    fn test_export_names() {
        let mut module = PlatformModule::new("Stdout".to_string());
        module.add_export(
            "line".to_string(),
            ModuleExport::function_export("line".to_string(), "...".to_string()),
        );
        module.add_export(
            "write".to_string(),
            ModuleExport::function_export("write".to_string(), "...".to_string()),
        );

        let names = module.export_names();
        assert_eq!(names.len(), 2);
    }
}
