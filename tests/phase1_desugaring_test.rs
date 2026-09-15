//! Phase 1 Desugaring Tests
//!
//! Tests for shorthand syntax desugaring before parsing.
//!
//! IMPORTANT: a trailing `!` IS part of the identifier — it is not an operator
//! and not sugar. Upstream's `chompIdentGeneral`
//! (roc-compiler/src/parse/tokenize.zig) chomps it into the name, and an
//! effectful binding whose name lacks `!` is only a warning
//! (roc-compiler/src/check/problem/types.zig `EffectfulFunctionName`).
//! So desugaring must leave `main!`, `echo!` and `Stdout.line!(..)` untouched.
//!
//! Likewise `=>` is the effectful function type in an annotation and the arm
//! separator in `match`. It is never rewritten to `->`.

use rocflight::desugaring::Desugarer;

fn desugar(input: &str) -> String {
    Desugarer::new(input.to_string()).desugar().unwrap()
}

#[test]
fn test_bang_preserved_in_definition() {
    let result = desugar("main! = \"Hello\"");

    assert_eq!(result, "main! = \"Hello\"");
}

#[test]
fn test_effect_arrow_not_rewritten() {
    // `=>` must never become `->`, and the annotation survives desugaring: the
    // emitted .roc has to pass `roc check` with its types explicit.
    let result = desugar("main! : Str => Str\nmain! = |s| s");

    assert!(!result.contains("->"), "rewrote => to ->: {}", result);
    assert_eq!(result, "main! : Str => Str\nmain! = |s| s");
}

#[test]
fn test_bang_preserved_in_calls() {
    let result = desugar("echo! = |msg| Stdout.line!(msg)");

    // Regression: the old effect pass emitted Rust here —
    // `match Stdout.line(msg) { Ok(v) => v, Err(e) => return Err(e) }`.
    assert_eq!(result, "echo! = |msg| Stdout.line!(msg)");
    assert!(!result.contains("return Err"), "emitted Rust: {}", result);
}

#[test]
fn test_desugar_preserves_strings() {
    let result = desugar(r#"main! = "There are ${birds} birds!""#);

    assert_eq!(result, r#"main! = "There are ${birds} birds!""#);
}

#[test]
fn test_operators_not_mangled() {
    // Regression: `!=` used to collapse to `=`, and unary `!` was deleted
    // outright, silently inverting the logic.
    let result = desugar("x = a != b\ny = !a");

    assert!(result.contains("a != b"), "mangled !=: {}", result);
    assert!(result.contains("!a"), "dropped unary !: {}", result);
}

#[test]
fn test_match_arms_not_rewritten() {
    let input = "z = match c {\n\tRed => 1\n\t_ => 2\n}";
    let result = desugar(input);

    // Match arms use `=>`; rewriting them to `->` produced invalid Roc.
    assert_eq!(result, input);
}

#[test]
fn test_desugar_complex_file() {
    let input = r#"
app [main!] { pf: platform "https://..." }

import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${birds} birds.")
"#;

    let result = desugar(input);

    // Entry point keeps its name, indentation is preserved, nothing invented.
    assert!(result.contains("app [main!]"), "{}", result);
    assert!(result.contains("main! = |_args|"), "{}", result);
    assert!(result.contains("  Stdout.line!("), "lost indentation: {:?}", result);
    assert!(result.contains("birds = -3"), "{}", result);
}

#[test]
fn test_desugarer_from_file() {
    let dir = std::env::temp_dir().join("rocflight_desugar_test");
    std::fs::create_dir_all(&dir).expect("Failed to create temp dir");
    let test_path = dir.join("test_desugar.roc");
    std::fs::write(&test_path, "echo! = |msg| \"Message: ${msg}\"").expect("Failed to write");

    let desugarer =
        Desugarer::from_file(test_path.to_str().unwrap()).expect("Failed to load file");
    let result = desugarer.desugar().expect("Failed to desugar");

    assert_eq!(result, "echo! = |msg| \"Message: ${msg}\"");

    let _ = std::fs::remove_file(&test_path);
}

#[test]
fn test_type_annotations_always_preserved() {
    // A bare annotation with no following binding is kept as-is.
    let result = desugar("factorial : U64 -> U64");
    assert_eq!(result, "factorial : U64 -> U64");

    // And it is still kept when the binding follows. The desugared file is a real
    // Roc program whose whole point is that the types are written out; the parser
    // skips annotation lines instead of the desugarer deleting them.
    let result = desugar("factorial : U64 -> U64\nfactorial = |n| n");
    assert_eq!(result, "factorial : U64 -> U64\nfactorial = |n| n");
}
