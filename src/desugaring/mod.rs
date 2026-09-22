//! Step 1 of the pipeline: a text-level rewrite before parsing.
//!
//! It is currently the identity. Everything that once lived here either was never
//! sugar or needs an expression's extent, so the parser does it — `??`, `?` (which
//! moves the rest of the block into the `Ok` arm), `.?` and `?:`. Learning.md §8 has
//! the full table, including the forms that only look like sugar. The tests below are
//! the guardrail: a text pass that scanned for `!` once emitted Rust-shaped nonsense
//! into `.roc` output, mangled `a != b` into `a = b` and rewrote every `match` arm's
//! `=>` to `->`.
//!
//! Whatever this emits must be a real Roc program that passes `roc check` on its own,
//! with its types explicit — that is what a golden pair's `.desugared.roc` file is,
//! which is why annotations are PRESERVED rather than stripped. The parser skips
//! annotation lines instead (`Parser::skip_type_annotation`).

use crate::error::ParseError;

pub struct Desugarer {
    input: String,
}

impl Desugarer {
    pub fn new(input: String) -> Self {
        Desugarer { input }
    }

    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        Ok(Desugarer { input: std::fs::read_to_string(path)? })
    }

    /// Desugar the source text. See the module header.
    pub fn desugar(&self) -> Result<String, ParseError> {
        // ponytail: no text-level pass remains. Kept as a step so the shape is obvious
        // if one is ever needed again; adding one means re-reading the module header.
        Ok(self.input.clone())
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
