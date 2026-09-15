//! Shorthand Syntax Desugaring
//!
//! Converts all Roc shorthand syntax to explicit functional syntax
//! BEFORE parsing. This keeps the parser simple and the AST clean.
//!
//! See DESUGARING.md for detailed rules on each transformation.
//!
//! Type annotations are PRESERVED, not stripped: the emitted .roc file is a real
//! Roc program that must pass `roc check` on its own, with explicit types (see
//! PHASE_IMPLEMENTATION_GUIDE.md, "The golden-pair rule"). Skipping annotations is
//! the parser's job, not the desugarer's.
//!
//! Handled in the PARSER, not here (they need the expression structure):
//! - `??` — default value; becomes `match e { Ok(v) => v, Err(_) => d }`
//! - `?` — error propagation; becomes a match whose Ok arm holds the rest of the block
//!
//! Not implemented:
//! - `.?` — optional field access; SEGFAULTS `roc` on nightly-2026-09-03, so no
//!   golden pair can be written against it
//! - `?:` — optional record fields; type-level only, and needs nominal types
//!
//! NOT desugaring, despite looking like it (see roc-compiler/src/parse/tokenize.zig
//! `chompIdentGeneral` and src/check/problem/types.zig `EffectfulFunctionName`):
//! - `foo!` — the `!` is part of the identifier. An effectful binding whose name
//!   lacks `!` is a warning upstream; there is nothing here to rewrite.
//! - `!foo` — unary logical not. Upstream canonicalizes it to a `Bool.not` call
//!   (src/base/mod.zig `CalledVia.unary_op`), not a text rewrite.
//! - `=>` — effectful function type in an annotation, and the arm separator in
//!   `match`. It is never `->`.

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
    /// Desugar the source text.
    ///
    /// Text-level passes only. `?` and `??` are NOT here: they are desugared in the
    /// parser, which is the only place with the information they need.
    ///
    /// * `??` becomes `match e { Ok(v) => v, Err(_) => default }` — a local rewrite,
    ///   but it needs the operand's extent, which means expression parsing.
    /// * `?` moves THE REST OF THE BLOCK into the `Ok` arm. Locating "the rest of the
    ///   block" in raw text is not something string substitution can do reliably;
    ///   the parser already builds blocks by folding statements from the end, so the
    ///   continuation is exactly what it has in hand.
    ///
    /// See `Parser::propagate_error` and `Parser::parse_or_expr`.
    pub fn desugar(&self) -> Result<String, ParseError> {
        // Type annotations are preserved; the parser skips them. See Rule 1 in
        // DESUGARING.md for why deleting them was wrong.
        let annotated = self.remove_type_annotations(&self.input)?;

        // ponytail: no text-level passes remain. Kept as a single call so the shape
        // is obvious when `.?` / `?:` arrive — and `.?` is blocked upstream anyway,
        // it segfaults `roc` on nightly-2026-09-03.
        Ok(annotated)
    }

    /// Pass 0: type annotations are kept verbatim.
    ///
    /// This used to delete `x : Type` lines so the parser never saw them. That made
    /// the emitted .roc file un-compilable as Roc and lost the types the desugared
    /// output is supposed to make explicit. The parser skips annotation lines
    /// instead; see `Parser::skip_type_annotation`.
    fn remove_type_annotations(&self, input: &str) -> Result<String, ParseError> {
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

    /// `!` is part of the identifier upstream (tokenize.zig `chompIdentGeneral`),
    /// so desugaring must leave both definitions and calls alone.
    #[test]
    fn test_bang_names_are_preserved() {
        let input = "echo! = |msg| Stdout.line!(msg)".to_string();
        let result = Desugarer::new(input).desugar().unwrap();

        assert_eq!(result, "echo! = |msg| Stdout.line!(msg)");
    }

    /// Regression: the old effect pass emitted Rust (`match c { Ok(v) => v, Err(e) => return Err(e) }`,
    /// comma-separated arms and all) for every `!` call, mangled `!=` into `=`,
    /// deleted unary `!`, and rewrote `match` arms' `=>` to `->`.
    #[test]
    fn test_no_rust_emitted_and_operators_intact() {
        let input = "\
x = a != b
y = !a
z = match c {
\tRed => 1
\t_ => 2
}"
        .to_string();
        let result = Desugarer::new(input).desugar().unwrap();

        assert!(!result.contains("return Err(e)"), "emitted Rust: {}", result);
        assert!(result.contains("a != b"), "mangled !=: {}", result);
        assert!(result.contains("!a"), "dropped unary !: {}", result);
        assert!(result.contains("Red => 1"), "rewrote match arm: {}", result);
    }

    /// `=>` marks an effectful function type; it is not `->`.
    #[test]
    fn test_effect_arrow_not_rewritten() {
        let input = "effect_demo! : Str => {}\neffect_demo! = |msg| echo!(msg)".to_string();
        let result = Desugarer::new(input).desugar().unwrap();

        // Nothing becomes `->`, and the annotation survives: the emitted .roc has to
        // compile on its own, with its types explicit.
        assert!(!result.contains("->"), "rewrote => to ->: {}", result);
        assert_eq!(result, "effect_demo! : Str => {}\neffect_demo! = |msg| echo!(msg)");
    }

    /// The desugared output is a real Roc program: annotations must survive so the
    /// emitted file passes `roc check` with explicit types.
    #[test]
    fn test_type_annotations_preserved() {
        let input = "app [main!] {}\n\nn : I64\nn = 42\n".to_string();
        let result = Desugarer::new(input).desugar().unwrap();

        assert!(result.contains("n : I64"), "stripped annotation: {:?}", result);
    }

    /// Roc is indentation-sensitive; Pass 1 used to trim every line.
    #[test]
    fn test_indentation_preserved() {
        let input = "main! = |_| {\n\tx = 1\n\tx\n}".to_string();
        let result = Desugarer::new(input).desugar().unwrap();

        assert!(result.contains("\n\tx = 1"), "lost indentation: {:?}", result);
    }
}
