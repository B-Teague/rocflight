//! Record update and destructuring — the rest of phase 09.
//!
//! Verified against `roc` nightly-2026-09-03:
//!   * the update spelling is `{ ..base, field: value }`. `{ base & field: value }` is
//!     REJECTED — it appears in the reference file only inside a commented-out TODO
//!   * `{ x, y } = r` destructures; inside a PATTERN a bare name means `x: x`
//!   * in a record LITERAL a bare name is NOT punning: `{ name }` is a block whose
//!     value is `name`
//!   * `..rest` in a pattern binds every field not named, so naming one `_` and
//!     capturing the rest removes it

use rocflight::desugaring::Desugarer;
use rocflight::eval::{Evaluator, Value};
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr<'static> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

/// Evaluate and unwrap a `Str` result, so `Str.inspect(...)` output compares as the
/// text it produced rather than a quoted Rust rendering of it.
fn as_str(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    match Evaluator::new().eval(&ast).expect("eval failed") {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    }
}

fn eval_error(src: &str) -> String {
    let ast = build(src);
    let _ = TypeChecker::new().synth(&ast);
    Evaluator::new().eval(&ast).expect_err("expected an error").message
}

fn accepts(src: &str) -> bool {
    TypeChecker::new().synth(&build(src)).is_ok()
}

// --- update ---------------------------------------------------------------

#[test]
fn an_update_replaces_the_named_fields() {
    assert_eq!(
        as_str("p = { name: \"ada\", age: 30 }\nStr.inspect({ ..p, age: 31 })"),
        "{ age: 31, name: \"ada\" }"
    );
}

#[test]
fn unnamed_fields_are_carried_over() {
    assert_eq!(
        as_str("p = { a: 1, b: 2, c: 3 }\nStr.inspect({ ..p, b: 9 })"),
        "{ a: 1, b: 9, c: 3 }"
    );
}

#[test]
fn the_base_is_not_mutated() {
    // Records are values; an update builds a new one.
    assert_eq!(
        as_str("p = { a: 1 }\nq = { ..p, a: 2 }\nStr.inspect(p)"),
        "{ a: 1 }"
    );
}

#[test]
fn several_fields_may_be_updated_at_once() {
    assert_eq!(
        as_str("p = { a: 1, b: 2 }\nStr.inspect({ ..p, a: 9, b: 8 })"),
        "{ a: 9, b: 8 }"
    );
}

#[test]
fn updating_a_field_the_record_lacks_is_an_error() {
    // An update cannot ADD a field.
    assert!(
        !accepts("p : { a: I64 }\np = { a: 1 }\n{ ..p, nope: 2 }"),
        "adding a field via update should fail"
    );
}

#[test]
fn updating_a_non_record_is_an_error() {
    let err = eval_error("x = 5\n{ ..x, a: 1 }");
    assert!(err.contains("not a record"), "got {}", err);
}

#[test]
fn an_update_is_still_distinguished_from_a_block() {
    // `{ ..base, ... }` starts with `..`, which no block statement does.
    assert_eq!(as_str("p = { a: 1 }\nStr.inspect({ ..p, a: 2 })"), "{ a: 2 }");
    // A real block still works.
    assert_eq!(as_str("f = |_| {\n    x = 1\n    x\n}\nI64.to_str(f(0))"), "1");
}

// --- destructuring --------------------------------------------------------

#[test]
fn a_record_destructuring_binds_its_fields() {
    assert_eq!(
        as_str("f = |_| {\n    { name, age } = { name: \"ada\", age: 30 }\n    \"${name}${I64.to_str(age)}\"\n}\nf(0)"),
        "ada30"
    );
}

#[test]
fn a_field_may_be_renamed_while_destructuring() {
    assert_eq!(
        as_str("f = |_| {\n    { name: who } = { name: \"ada\" }\n    who\n}\nf(0)"),
        "ada"
    );
}

#[test]
fn destructuring_works_at_the_top_level() {
    assert_eq!(as_str("p = { a: 1, b: 2 }\n{ a, b } = p\nI64.to_str(a + b)"), "3");
}

// --- ..rest ---------------------------------------------------------------

#[test]
fn rest_collects_the_fields_not_named() {
    assert_eq!(
        as_str("f = |p| {\n    { email: _, ..rest } = p\n    Str.inspect(rest)\n}\nf({ name: \"ada\", age: 30, email: \"e\" })"),
        "{ age: 30, name: \"ada\" }"
    );
}

#[test]
fn rest_may_be_empty() {
    assert_eq!(
        as_str("f = |p| {\n    { a: _, ..rest } = p\n    Str.inspect(rest)\n}\nf({ a: 1 })"),
        "{}"
    );
}

#[test]
fn named_fields_still_bind_alongside_rest() {
    assert_eq!(
        as_str("f = |p| {\n    { a, ..rest } = p\n    \"${I64.to_str(a)}${Str.inspect(rest)}\"\n}\nf({ a: 1, b: 2 })"),
        "1{ b: 2 }"
    );
}

// --- the literal/pattern asymmetry ----------------------------------------

#[test]
fn a_bare_name_puns_in_a_pattern_but_not_in_a_literal() {
    // In a PATTERN, `{ name }` means `{ name: name }`.
    assert_eq!(
        as_str("f = |_| {\n    { name } = { name: \"ada\" }\n    name\n}\nf(0)"),
        "ada"
    );
    // In an EXPRESSION, `{ name }` is a block whose value is `name` — verified against
    // roc, which prints the string rather than a record.
    assert_eq!(as_str("name = \"ada\"\nStr.inspect({ name })"), "\"ada\"");
}
