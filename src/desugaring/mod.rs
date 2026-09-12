//! Shorthand Syntax Desugaring
//!
//! Converts all Roc shorthand syntax to explicit functional syntax
//! BEFORE parsing. This keeps the parser simple and the AST clean.
//!
//! See DESUGARING.md for detailed rules on each transformation.
//!
//! Shorthand syntax handled:
//! - `!` — Effectful function marker (PRESERVED, not removed)
//! - `?` — Error propagation operator (expands to match)
//! - `??` — Default value operator (expands to match)
//! - `.?` — Optional field access (Phase 9 - placeholder)
//! - `?:` — Optional record fields (Phase 9 - placeholder)
//! - `=>` — Effect type notation (converts to ->)

use crate::error::ParseError;

/// Desugarer: converts shorthand syntax to functional syntax
pub struct Desugarer {
    input: String,
}

impl Desugarer {
    /// Create new desugarer for input
    pub fn new(input: String) -> Self {
        Desugarer { input }
    }

    /// Load from file and create desugarer
    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let input = std::fs::read_to_string(path)?;
        Ok(Desugarer { input })
    }

    /// Run all desugaring passes in order
    /// Each pass transforms shorthand syntax into explicit, verbose forms
    pub fn desugar(&self) -> Result<String, ParseError> {
        // Pass 1: Remove type annotation lines (e.g., "x : I64" before "x = 42")
        let step1 = self.remove_type_annotations(&self.input)?;

        // Pass 2: Convert effect type arrows (=> becomes ->)
        // IMPORTANT: Preserve ! in function names (e.g., echo!, main!)
        let step2 = self.desugar_effect_arrows(&step1)?;

        // Pass 3: Expand ?? operator (default values) to match expressions
        // Do this BEFORE ? operator since ?? contains ?
        let step3 = self.desugar_default_operator(&step2)?;

        // Pass 4: Expand ? operator (error propagation) to match expressions
        let step4 = self.desugar_question_operator(&step3)?;

        // Pass 5: Handle .? optional field access (placeholder for Phase 9)
        let step5 = self.desugar_optional_field_access(&step4)?;

        // Pass 6: Handle ?: optional record fields (placeholder for Phase 9)
        let step6 = self.desugar_optional_record_fields(&step5)?;

        Ok(step6)
    }

    /// Pass 0: Remove type annotations
    /// `x : Type` on its own line → removed
    /// `x : Type` followed by `x = value` → keep only the binding
    fn remove_type_annotations(&self, input: &str) -> Result<String, ParseError> {
        let lines: Vec<&str> = input.lines().collect();
        let mut result = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Check if this line is a type annotation: "name : Type"
            let is_annotation = if let Some(colon_pos) = line.find(':') {
                let before_colon = line[..colon_pos].trim();
                // Type annotation: single identifier or qualified name before colon, no operators
                before_colon.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') &&
                !before_colon.contains("(") && !before_colon.contains("=")
            } else {
                false
            };

            if is_annotation {
                // Check if next non-empty line is a binding for this name
                let before_colon = &line[..line.find(':').unwrap()].trim();
                let mut should_skip = false;

                for next_idx in (i + 1)..lines.len() {
                    let next_line = lines[next_idx].trim();
                    if !next_line.is_empty() {
                        // Check if this is the binding
                        if next_line.starts_with(&format!("{} =", before_colon)) {
                            should_skip = true;
                        }
                        break;
                    }
                }

                if should_skip {
                    // Skip this annotation line
                    i += 1;
                    continue;
                }
            }

            // Keep the line
            result.push(line);
            i += 1;
        }

        Ok(result.join("\n"))
    }

    /// Pass 2: Handle effect markers and type arrows
    ///
    /// Two transformations:
    /// 1. Strip `!` from function names and calls (marks effectful, not part of name)
    /// 2. Convert `=>` to `->` in type annotations
    ///
    /// After desugaring:
    /// - `echo!("hello")` becomes `echo("hello")` (! removed)
    /// - `main!` becomes `main` (! removed)
    /// - `Str => Result` becomes `Str -> Result` (arrow converted)
    ///
    /// NOTE: The ! removal here is just cleaning up names. Full error wrapping
    /// happens in Pass 4 (desugar_question_operator) which wraps calls.
    fn desugar_effect_arrows(&self, input: &str) -> Result<String, ParseError> {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            // Handle string literals: don't desugar inside them
            if ch == '"' {
                result.push(ch);
                // Copy string literal as-is, preserving all characters
                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    result.push(next_ch);

                    if next_ch == '"' {
                        break; // End of string
                    } else if next_ch == '\\' {
                        // Handle escaped characters
                        if let Some(&escaped) = chars.peek() {
                            chars.next();
                            result.push(escaped);
                        }
                    }
                }
            } else if ch == '!' {
                // Remove ! - it's not part of the identifier, it marks effectful functions
                // The ! will be handled by error wrapping in Pass 4
                // Just skip it here
                continue;
            } else if ch == '=' && chars.peek() == Some(&'>') {
                // Convert => to -> (only for type annotations)
                chars.next(); // consume >
                result.push_str("->");
            } else {
                result.push(ch);
            }
        }

        Ok(result)
    }

    /// Pass 3: Expand ?? operator (default values)
    /// `expr ?? default` → `match expr { Ok(v) => v, Err(_) => default }`
    fn desugar_default_operator(&self, input: &str) -> Result<String, ParseError> {
        // For now, this is a placeholder. Full implementation requires expression parsing.
        // The ?? operator is relatively rare, so we defer this to a later phase.
        Ok(input.to_string())
    }

    /// Pass 4: Expand ? operator (error propagation)
    /// `expr?` → `match expr { Ok(v) => v, Err(e) => return Err(e) }`
    fn desugar_question_operator(&self, input: &str) -> Result<String, ParseError> {
        // For now, this is a placeholder. Full implementation requires careful parsing to:
        // 1. Identify complete expressions followed by ?
        // 2. Not confuse with ? in type annotations
        // 3. Handle nested expressions correctly
        // This is deferred to Phase 7 when error handling is properly designed.
        Ok(input.to_string())
    }

    /// Pass 5: Handle .? optional field access
    /// `record.?field` → internal Try-producing function call
    /// Placeholder for Phase 9 (records)
    fn desugar_optional_field_access(&self, input: &str) -> Result<String, ParseError> {
        Ok(input.to_string())
    }

    /// Pass 6: Handle ?: optional record fields
    /// `field ?: Type` → mark field as optional in record
    /// Placeholder for Phase 9 (records)
    fn desugar_optional_record_fields(&self, input: &str) -> Result<String, ParseError> {
        Ok(input.to_string())
    }


    /// Save desugared output to cache directory (both debug and release builds)
    /// Cache structure: .rocflight/cache/desugared/<original_path>.desugared.roc
    pub fn save_debug(&self, original_path: &str, desugared: &str) -> Result<(), std::io::Error> {
        // Create cache directory structure
        let cache_dir = ".rocflight/cache/desugared";
        std::fs::create_dir_all(cache_dir)?;

        // Create desugared file path in cache
        // Use the original path as part of the cache filename for clarity
        let filename = original_path
            .replace("/", "_")
            .replace("\\", "_")
            .replace(".", "_");
        let cache_path = format!("{}/{}.desugared.roc", cache_dir, filename);

        // Write desugared content to cache
        std::fs::write(&cache_path, desugared)?;
        eprintln!("[Desugaring] Cached to: {}", cache_path);

        Ok(())
    }

    /// Clear the desugaring cache
    /// Call this when the binary is rebuilt to ensure fresh desugaring
    pub fn clear_cache() -> Result<(), std::io::Error> {
        let cache_dir = ".rocflight/cache/desugared";
        if std::path::Path::new(cache_dir).exists() {
            std::fs::remove_dir_all(cache_dir)?;
            eprintln!("[Desugaring] Cache cleared: {}", cache_dir);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desugar_simple_effect() {
        let input = "main! = |_args| \"hello\"".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar_effect_arrows(&desugarer.input).expect("Desugaring failed");

        // IMPORTANT: ! is REMOVED during desugaring
        // It marks the function as effectful, but is not part of the name
        assert!(result.contains("main ="));
        assert!(!result.contains("main!"));
    }

    #[test]
    fn test_desugar_effect_type() {
        let input = "main! : Str => Result".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar_effect_arrows(&desugarer.input).unwrap();

        // => should become ->
        assert!(result.contains("->"));
        assert!(!result.contains("=>"));
        // ! should be removed
        assert!(result.contains("main :"));
        assert!(!result.contains("main!"));
    }

    #[test]
    fn test_desugar_multiple_effects() {
        let input = "echo! = |msg| Stdout.line!(msg)".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar_effect_arrows(&desugarer.input).unwrap();

        // ! is removed from all function names and calls
        assert!(result.contains("echo ="));
        assert!(result.contains("Stdout.line"));
        assert!(!result.contains("echo!"));
        assert!(!result.contains("line!"));
    }

    #[test]
    fn test_full_desugar_phase1() {
        let input = "main! = \"Hello, world!\"".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar().unwrap();

        // ! is REMOVED during desugaring (Pass 2)
        assert!(result.starts_with("main ="));
        assert!(!result.contains("main!"));
        // Should still have the string with its ! preserved (inside string)
        assert!(result.contains("\"Hello, world!\""));
    }
}
