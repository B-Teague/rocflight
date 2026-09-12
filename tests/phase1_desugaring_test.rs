//! Phase 1 Desugaring Tests
//!
//! Tests for shorthand syntax desugaring before parsing
//!
//! IMPORTANT: ! is NOT part of the function name!
//! The ! is a postfix operator marking effectful functions.
//! During desugaring, ! is REMOVED from function names and calls.
//! The full error handling wrapping happens in Pass 4 (Phase 7).

use rocflight::desugaring::Desugarer;

#[test]
fn test_desugar_simple_effect() {
    let input = "main! = \"Hello\"".to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // ! is REMOVED during desugaring
    assert!(result.contains("main ="));
    assert!(!result.contains("main!"));
    assert!(result.contains("\"Hello\""));
}

#[test]
fn test_desugar_effect_type_notation() {
    let input = "main! : Str => Result".to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // => converts to ->
    assert!(!result.contains("=>"));
    assert!(result.contains("->"));
    // ! is REMOVED
    assert!(result.contains("main :"));
    assert!(!result.contains("main!"));
}

#[test]
fn test_desugar_multiple_effects() {
    let input = "echo! = |msg| Stdout.line!(msg)".to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // ! is REMOVED from all function names and calls
    assert!(result.contains("echo ="));
    assert!(result.contains("Stdout.line"));
    assert!(!result.contains("echo!"));
    assert!(!result.contains("line!"));
}

#[test]
fn test_desugar_preserves_strings() {
    let input = r#"main! = "There are ${birds} birds!""#.to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // String ! is preserved (inside string)
    assert!(result.contains("There are"));
    assert!(result.contains("birds"));
    assert!(result.contains("!\""));
    // Function name ! is REMOVED
    assert!(result.contains("main ="));
    assert!(!result.contains("main!"));
}

#[test]
fn test_desugar_complex_file() {
    let input = r#"
app [main!] { pf: platform "https://..." }

import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${birds} birds.")
"#
    .to_string();

    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // ! is REMOVED from function names
    assert!(result.contains("app [main]"));
    assert!(result.contains("main ="));
    assert!(result.contains("Stdout.line"));
    assert!(!result.contains("main!"));
    assert!(!result.contains("line!"));

    // Structure should be preserved
    assert!(result.contains("birds"));
}

#[test]
fn test_desugarer_from_file() {
    // Create test file
    let test_content = "echo! = |msg| \"Message: ${msg}\"";
    let test_path = "/tmp/test_desugar.roc";
    std::fs::write(test_path, test_content).expect("Failed to write test file");

    // Load and desugar
    let desugarer = Desugarer::from_file(test_path).expect("Failed to load file");
    let result = desugarer.desugar().expect("Failed to desugar");

    // ! is REMOVED
    assert!(result.contains("echo ="));
    assert!(!result.contains("echo!"));
    assert!(result.contains("msg"));

    // Clean up
    let _ = std::fs::remove_file(test_path);
}

#[test]
fn test_desugar_preserves_non_effect_identifiers() {
    let input = "factorial! = |n| n".to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // ! is REMOVED from the function name
    assert!(result.contains("factorial ="));
    assert!(!result.contains("factorial!"));
    // Parameter is preserved
    assert!(result.contains("n"));
}

#[test]
fn test_multiple_desugaring_passes() {
    let input = "main! : Str => Str\nmain! = |s| s".to_string();
    let desugarer = Desugarer::new(input);
    let result = desugarer.desugar().unwrap();

    // Type arrow converted
    assert!(result.contains("->"));
    assert!(!result.contains("=>"));
    // ! REMOVED from function name
    assert!(result.contains("main ="));
    assert!(!result.contains("main!"));
    assert!(result.contains("Str"));
}
