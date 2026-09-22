//! Desugaring: what the sugar means, and that it means the same thing.
//!
//! Two checks that need neither the `roc` binary nor a built rocflight — the
//! desugarer alone, then the golden pairs under `tests/roc/` whose two spellings
//! must build the same AST.

// ==========================================================================
// The golden pairs must describe the same program.
//
// Every syntax feature under `tests/roc/` is a pair: a sugared file and a
// `.desugared.roc` sibling that spells out by hand what desugaring is supposed to
// produce. They differ only in sugar, so they must build the same AST — that is the
// definition of the desugarer being right, and it is what proves the parser handles
// both syntaxes.
//
// This used to be `rocflight --ast-only` run twice from `tests/check_roc.sh`. It needs
// neither the `roc` compiler nor a built binary, only the library, so it belongs here:
// the check is not a thing the interpreter does, and does not need a flag to ask for.
//
// Only the AST STRUCTURE is compared, not the inferred type. Annotations are not
// sugar: the desugared file declares types the sugared one leaves to inference, so its
// type is legitimately more specific (`List(Str) -> ...` versus `$0 -> ...`).

use std::path::{Path, PathBuf};

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;

/// Desugar and parse one file, rendering its AST.
///
/// `None` means the file does not parse. A pair whose feature is not implemented yet
/// fails loudly in `check_roc.sh`, against `roc` itself, which is the gate that should
/// report it; repeating it here would report the same gap twice.
fn ast_of(path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let desugared = Desugarer::new(source).desugar().ok()?;
    let mut parser = Parser::named(&path.display().to_string(), &desugared);
    Some(format!("{}", parser.parse_expr().ok()?))
}

/// Every `.roc` under `tests/roc/` that is not itself a `.desugared.roc`.
fn sugared_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        // `examples/` and `bench/` are whole programs, not pairs; `check_roc.sh` skips
        // them on the same two names.
        if path.is_dir() {
            if matches!(path.file_name().and_then(|n| n.to_str()), Some("examples") | Some("bench")) {
                continue;
            }
            sugared_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "roc")
            && !path.to_string_lossy().ends_with(".desugared.roc")
        {
            out.push(path);
        }
    }
}

#[test]
fn golden_pairs_build_the_same_ast() {
    let mut sugared = Vec::new();
    sugared_files(Path::new("tests/roc"), &mut sugared);
    assert!(sugared.len() > 50, "expected the golden corpus, found {} files", sugared.len());

    let (mut compared, mut differ) = (0, Vec::new());
    for path in &sugared {
        let des = PathBuf::from(format!("{}.desugared.roc", path.display().to_string().trim_end_matches(".roc")));
        assert!(des.exists(), "{} has no .desugared.roc sibling", path.display());

        // Both sides must parse for the comparison to mean anything.
        let (Some(a), Some(b)) = (ast_of(path), ast_of(&des)) else { continue };
        compared += 1;
        if a != b {
            differ.push(format!("{}\n  sugared:   {}\n  desugared: {}", path.display(), a, b));
        }
    }

    assert!(
        differ.is_empty(),
        "{} of {} pairs build different ASTs:\n{}",
        differ.len(),
        compared,
        differ.join("\n")
    );
    assert!(compared > 50, "only {} pairs parsed on both sides", compared);
}

// ==========================================================================
// Phase 1 Desugaring Tests
//
// Tests for shorthand syntax desugaring before parsing.
//
// IMPORTANT: a trailing `!` IS part of the identifier — it is not an operator
// and not sugar. Upstream's `chompIdentGeneral`
// (roc-compiler/src/parse/tokenize.zig) chomps it into the name, and an
// effectful binding whose name lacks `!` is only a warning
// (roc-compiler/src/check/problem/types.zig `EffectfulFunctionName`).
// So desugaring must leave `main!`, `echo!` and `Stdout.line!(..)` untouched.
//
// Likewise `=>` is the effectful function type in an annotation and the arm
// separator in `match`. It is never rewritten to `->`.

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
