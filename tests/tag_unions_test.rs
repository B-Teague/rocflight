//! Tag unions — phase 12.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `Str.inspect` renders a bare tag as `Red` and a payload tag as `Foo(42, "hi")`
//!   * tags compare by name, and by payload when they have one
//!   * `Try(a, b)` is `[Ok(a), Err(b)]` under the hood
//!   * `[Red, Green, ..]` openness applies to parameter positions, not to a value
//!     binding: `c : [Red, Green, ..]` still rejects `c = Blue`

use rocflight::desugaring::Desugarer;
use rocflight::eval::{Evaluator, Value};
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn ty(src: &str) -> String {
    TypeChecker::new().synth(&build(src)).expect("type check failed").to_string()
}

fn eval(src: &str) -> Value {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    Evaluator::new().eval(&ast).expect("eval failed")
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

// --- construction ---------------------------------------------------------

#[test]
fn bare_tag_is_a_value_not_a_call() {
    match eval("Red") {
        Value::Tag(name, payload) => {
            assert_eq!(name, "Red");
            assert!(payload.is_empty(), "bare tag should have no payload");
        }
        other => panic!("expected Tag, got {:?}", other),
    }
}

#[test]
fn tag_can_carry_payloads() {
    match eval(r#"Foo(42, "hi")"#) {
        Value::Tag(name, payload) => {
            assert_eq!(name, "Foo");
            assert_eq!(payload.len(), 2);
        }
        other => panic!("expected Tag, got {:?}", other),
    }
}

// --- typing ---------------------------------------------------------------

#[test]
fn a_tag_literal_is_an_open_one_tag_union() {
    // Open — rendered with a trailing `..`. A tag expression says "at least Red", so
    // unification may add more tags. Only an annotation produces a CLOSED union.
    assert_eq!(ty("Red"), "[Red, ..]");
    assert_eq!(ty(r#"Foo(1, "a")"#), "[Foo(I64, Str), ..]");
}

#[test]
fn branches_with_different_tags_join_into_a_union() {
    // The if's type is the union of its branches, not just the first one.
    assert_eq!(ty("if 1 == 1 Red else Green"), "[Green, Red, ..]");
}

#[test]
fn union_members_are_sorted_so_order_does_not_matter() {
    assert_eq!(
        ty("if 1 == 1 Red else if 1 == 2 Green else Blue"),
        "[Blue, Green, Red, ..]"
    );
    // Same tags written the other way round give the same type.
    assert_eq!(
        ty("if 1 == 1 Blue else if 1 == 2 Green else Red"),
        "[Blue, Green, Red, ..]"
    );
}

#[test]
fn the_same_tag_in_both_branches_does_not_duplicate() {
    assert_eq!(ty("if 1 == 1 Foo(1) else Foo(2)"), "[Foo(I64), ..]");
}

#[test]
fn payload_arity_mismatch_is_a_type_error() {
    let err = TypeChecker::new()
        .synth(&build("if 1 == 1 Foo(1) else Foo(1, 2)"))
        .expect_err("differing payload arity should fail");
    assert!(
        err.message.contains("payload"),
        "error should mention payloads, got {:?}",
        err.message
    );
}

#[test]
fn payload_type_mismatch_is_a_type_error() {
    assert!(
        TypeChecker::new().synth(&build(r#"if 1 == 1 Foo(1) else Foo("s")"#)).is_err(),
        "I64 vs Str payload should fail"
    );
}

#[test]
fn ok_and_err_are_ordinary_tags() {
    // `Try(a, b)` is `[Ok(a), Err(b)]`, so nothing special is needed for them.
    assert_eq!(ty("Ok({})"), "[Ok({}), ..]");
    assert_eq!(ty(r#"Err("boom")"#), "[Err(Str), ..]");
}

// --- equality -------------------------------------------------------------

#[test]
fn bare_tags_compare_by_name() {
    assert_eq!(as_str("Str.inspect(Red == Red)"), "True");
    assert_eq!(as_str("Str.inspect(Red == Green)"), "False");
}

#[test]
fn payload_tags_compare_payloads_too() {
    assert_eq!(as_str("Str.inspect(Foo(1) == Foo(1))"), "True");
    assert_eq!(as_str("Str.inspect(Foo(1) == Foo(2))"), "False");
    assert_eq!(as_str(r#"Str.inspect(Foo(1, "a") == Foo(1, "a"))"#), "True");
}

#[test]
fn a_tag_never_equals_a_non_tag() {
    assert_eq!(as_str("Str.inspect(Red == 1)"), "False");
}

// --- rendering ------------------------------------------------------------

#[test]
fn inspect_matches_roc_for_tags() {
    assert_eq!(as_str("Str.inspect(Red)"), "Red");
    assert_eq!(as_str(r#"Str.inspect(Foo(1, "hi"))"#), "Foo(1, \"hi\")");
    assert_eq!(as_str(r#"Str.inspect(Wrap("x"))"#), "Wrap(\"x\")");
    // Nested: payloads are inspected recursively, so strings stay quoted.
    assert_eq!(as_str(r#"Str.inspect(Outer(Inner("y")))"#), "Outer(Inner(\"y\"))");
}

#[test]
fn tags_carry_records_and_records_carry_tags() {
    assert_eq!(
        as_str("Str.inspect(Wrap({ b: Bool.True, a: Red }))"),
        "Wrap({ a: Red, b: True })"
    );
}
