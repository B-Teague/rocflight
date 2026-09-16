//! Phase 1 tests: String literals with type checking and evaluation

use rocflight::parser::Parser;
use rocflight::types::TypeChecker;
use rocflight::types::Type;

#[test]
fn test_parse_string_literal() {
    let mut parser = Parser::new("\"hello\"");
    let expr = parser.parse_expr().expect("Failed to parse");
    // Just verify it parses without error
    println!("Parsed: {:?}", expr);
}

#[test]
fn test_string_type() {
    let mut parser = Parser::new("\"hello\"");
    let expr = parser.parse_expr().expect("Failed to parse");

    let mut type_checker = TypeChecker::new();
    let ty = type_checker.synth(&expr).expect("Failed to infer type");

    assert_eq!(ty, Type::Str, "String literal should have type Str");
}

#[test]
fn test_eval_string() {
    let mut parser = Parser::new("\"hello\"");
    let expr = parser.parse_expr().expect("Failed to parse");

    let value = rocflight::vm::eval(&expr).expect("Failed to evaluate");

    println!("Evaluated to: {}", value);
}

#[test]
fn test_empty_string() {
    let mut parser = Parser::new("\"\"");
    let expr = parser.parse_expr().expect("Failed to parse");

    let mut type_checker = TypeChecker::new();
    let ty = type_checker.synth(&expr).expect("Failed to infer type");

    assert_eq!(ty, Type::Str);
}

#[test]
fn test_string_with_escapes() {
    let mut parser = Parser::new("\"hello\\nworld\"");
    let expr = parser.parse_expr().expect("Failed to parse");

    let mut type_checker = TypeChecker::new();
    let ty = type_checker.synth(&expr).expect("Failed to infer type");

    assert_eq!(ty, Type::Str);

    let value = rocflight::vm::eval(&expr).expect("Failed to evaluate");
    println!("String with escapes: {}", value);
}
