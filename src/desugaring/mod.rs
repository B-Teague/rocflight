//! Shorthand Syntax Desugaring
//!
//! Converts all Roc shorthand syntax to explicit functional syntax
//! BEFORE parsing. This keeps the parser simple and the AST clean.
//!
//! Shorthand syntax handled:
//! - `!` — Effectful function marker (Phase 1B)
//! - `?` — Error handling operator (Phase 7)
//! - `??` — Default value operator (Phase 7)
//! - `.?` — Optional field access (Phase 9)
//! - `?:` — Optional record fields (Phase 9)
//! - `=>` — Effect type notation (Phase 1B)

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

    /// Run all desugaring passes
    pub fn desugar(&self) -> Result<String, ParseError> {
        // Pass 1: Remove ! from effectful function names and type annotations
        let step1 = self.desugar_effects(&self.input)?;

        // Pass 2: Replace ? operators with match expressions
        let step2 = self.desugar_question_mark(&step1)?;

        // Pass 3: Replace ?? operators with match expressions
        let step3 = self.desugar_default(&step2)?;

        // Pass 4: Replace .? with Try-based access
        let step4 = self.desugar_optional_access(&step3)?;

        // Pass 5: Process optional fields (?:) in records
        let step5 = self.desugar_optional_fields(&step4)?;

        Ok(step5)
    }

    /// Pass 1: Remove ! from effectful function names
    /// `main!` → `main`
    /// `Stdout.line!` → `Stdout.line`
    /// Type `Str => Result` → `Str -> Result`
    /// BUT: Don't touch ! inside string literals
    fn desugar_effects(&self, input: &str) -> Result<String, ParseError> {
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
            } else if ch.is_alphabetic() || ch == '_' {
                // Identifier possibly followed by !
                let mut ident = String::from(ch);

                // Collect identifier
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        ident.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Check if followed by !
                if chars.peek() == Some(&'!') {
                    // Remove the !
                    chars.next(); // consume the !
                    result.push_str(&ident); // add identifier without !
                } else {
                    result.push_str(&ident);
                }
            } else if ch == '=' && chars.peek() == Some(&'>') {
                // Convert => to ->
                chars.next(); // consume >
                result.push_str("->");
            } else {
                result.push(ch);
            }
        }

        Ok(result)
    }

    /// Pass 2: Replace ? operator with match expressions
    /// `expr?` → `match expr { Ok(v) => v, Err(e) => return Err(e) }`
    fn desugar_question_mark(&self, input: &str) -> Result<String, ParseError> {
        // For now, this is a placeholder
        // Full implementation will replace expr? patterns with match expressions
        // Deferred to Phase 7 when error handling is implemented
        Ok(input.to_string())
    }

    /// Pass 3: Replace ?? operator with match expressions
    /// `expr ?? default` → `match expr { Ok(v) => v, Err(_) => default }`
    fn desugar_default(&self, input: &str) -> Result<String, ParseError> {
        // Placeholder - deferred to Phase 7
        Ok(input.to_string())
    }

    /// Pass 4: Replace .? with Try-based access
    /// `rec.?field` → special Try handling
    fn desugar_optional_access(&self, input: &str) -> Result<String, ParseError> {
        // Placeholder - deferred to Phase 9
        Ok(input.to_string())
    }

    /// Pass 5: Process optional fields in records
    /// `field ?: Type` → mark field as optional
    fn desugar_optional_fields(&self, input: &str) -> Result<String, ParseError> {
        // Placeholder - deferred to Phase 9
        Ok(input.to_string())
    }

    /// Save desugared output to temp file (debug builds)
    pub fn save_debug(&self, _original_path: &str, _desugared: &str) -> Result<(), std::io::Error> {
        #[cfg(debug_assertions)]
        {
            let temp_path = format!("{}.desugared.roc", _original_path);
            std::fs::write(&temp_path, _desugared)?;
            eprintln!("[Desugaring] Saved to: {}", temp_path);
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
        let result = desugarer.desugar_effects(&desugarer.input).unwrap();

        assert!(result.contains("main ="));
        assert!(!result.contains("main!"));
    }

    #[test]
    fn test_desugar_effect_type() {
        let input = "main! : Str => Result".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar_effects(&desugarer.input).unwrap();

        assert!(result.contains("->"));
        assert!(!result.contains("=>"));
    }

    #[test]
    fn test_desugar_multiple_effects() {
        let input = "echo! = |msg| Stdout.line!(msg)".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar_effects(&desugarer.input).unwrap();

        assert_eq!(result.matches("echo").count(), 1); // No echo! left
        assert_eq!(result.matches("line").count(), 1); // No line! left
        assert!(!result.contains("!"));
    }

    #[test]
    fn test_full_desugar_phase1() {
        let input = "main! = \"Hello, world!\"".to_string();
        let desugarer = Desugarer::new(input);
        let result = desugarer.desugar().unwrap();

        // After desugaring, the ! after identifier should be removed
        // (but ! inside strings should be preserved)
        assert!(result.starts_with("main = "));
        // Should still have the string with its ! preserved
        assert!(result.contains("\"Hello, world!\""));
    }
}
