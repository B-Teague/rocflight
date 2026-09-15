//! Error-handling sugar — phase 17: `?` and `??`.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `?` unwraps Ok and skips the rest of the block on Err
//!   * `?` on a block's FINAL expression is a type error — it unwraps, so the body
//!     yields the payload rather than a Try
//!   * `??` binds looser than arithmetic: `x ?? 1 + 2` is `x ?? (1 + 2)`, giving 3
//!
//! NOT implemented, with reasons:
//!   * `.?` optional field access — SEGFAULTS the roc compiler on nightly-2026-09-03,
//!     so no golden pair can be written for it
//!   * `?:` optional record fields — type-level only, and needs nominal types

use rocflight::desugaring::Desugarer;
use rocflight::eval::{Evaluator, Value};
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn parse(src: &str) -> Result<rocflight::ast::Expr<'static>, rocflight::error::ParseError> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr()
}

fn eval(src: &str) -> Value {
    let ast = parse(src).expect("parse failed");
    TypeChecker::new().synth(&ast).expect("type check failed");
    Evaluator::new().eval(&ast).expect("eval failed")
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

// --- ?? -------------------------------------------------------------------

#[test]
fn default_operator_takes_the_ok_value() {
    assert_eq!(as_str(r#"f = |s| I64.from_str(s) ?? 0
I64.to_str(f("42"))"#), "42");
}

#[test]
fn default_operator_falls_back_on_err() {
    assert_eq!(as_str(r#"f = |s| I64.from_str(s) ?? 7
I64.to_str(f("nope"))"#), "7");
}

#[test]
fn default_operator_binds_looser_than_arithmetic() {
    // `x ?? 1 + 2` is `x ?? (1 + 2)` = 3, not `(x ?? 1) + 2`.
    assert_eq!(as_str(r#"f = |s| I64.from_str(s) ?? 1 + 2
I64.to_str(f("nope"))"#), "3");
    // ...and the Ok path is unaffected by the default's shape.
    assert_eq!(as_str(r#"f = |s| I64.from_str(s) ?? 1 + 2
I64.to_str(f("9"))"#), "9");
}

#[test]
fn default_operator_desugars_to_a_match() {
    let ast = parse(r#"f = |s| I64.from_str(s) ?? 0
f"#).unwrap();
    let rendered = format!("{}", ast);
    assert!(
        rendered.contains("match") && rendered.contains("Ok(v)") && rendered.contains("Err(_)"),
        "?? should desugar to a match, got {}",
        rendered
    );
}

// --- ? --------------------------------------------------------------------

#[test]
fn question_unwraps_on_the_ok_path() {
    let src = r#"f = |s| {
    n = I64.from_str(s)?
    Ok(n + 1)
}
match f("41") { Ok(v) => I64.to_str(v) Err(_) => "err" }"#;
    assert_eq!(as_str(src), "42");
}

#[test]
fn question_short_circuits_the_rest_of_the_block() {
    // The `Ok(n + 1)` after the `?` must not run when from_str fails.
    let src = r#"f = |s| {
    n = I64.from_str(s)?
    Ok(n + 1)
}
match f("nope") { Ok(v) => I64.to_str(v) Err(_) => "err" }"#;
    assert_eq!(as_str(src), "err");
}

#[test]
fn question_chains() {
    let src = r#"add = |a, b| {
    x = I64.from_str(a)?
    y = I64.from_str(b)?
    Ok(x + y)
}
match add("1", "2") { Ok(v) => I64.to_str(v) Err(_) => "err" }"#;
    assert_eq!(as_str(src), "3");
    let bad = src.replace(r#"add("1", "2")"#, r#"add("1", "zz")"#);
    assert_eq!(as_str(&bad), "err");
}

#[test]
fn question_moves_the_continuation_into_the_ok_arm() {
    // This is what makes `?` non-trivial: the statements AFTER it end up inside the
    // Ok arm, so an Err skips them.
    let ast = parse(r#"f = |s| {
    n = I64.from_str(s)?
    Ok(n + 1)
}
f"#).unwrap();
    let rendered = format!("{}", ast);
    assert!(
        rendered.contains("Ok(n) => Ok((n + 1))"),
        "the continuation should sit inside the Ok arm, got {}",
        rendered
    );
    assert!(
        rendered.contains("Err(e) => Err(e)"),
        "the Err arm should re-wrap the error, got {}",
        rendered
    );
}

#[test]
fn question_without_a_binding_still_propagates() {
    // `_ = expr?` discards the value but keeps the short-circuit.
    let src = r#"f = |s| {
    _ = I64.from_str(s)?
    Ok("survived")
}
match f("1") { Ok(v) => v Err(_) => "err" }"#;
    assert_eq!(as_str(src), "survived");
    let bad = src.replace(r#"f("1")"#, r#"f("zz")"#);
    assert_eq!(as_str(&bad), "err");
}

#[test]
fn question_on_the_final_expression_is_rejected() {
    // roc rejects it too, as a type error: `?` unwraps, so the block yields the
    // payload rather than a Try. Here there is also no continuation to move.
    let err = parse("f = |s| {\n    I64.from_str(s)?\n}\nf")
        .expect_err("`?` as the final expression should not parse");
    assert!(
        err.message.contains('?'),
        "error should mention the operator, got {:?}",
        err.message
    );
}

#[test]
fn double_question_is_not_read_as_two_singles() {
    // `??` must be consumed by the expression parser before the block looks for `?`.
    assert_eq!(as_str(r#"f = |s| I64.from_str(s) ?? 5
I64.to_str(f("nope"))"#), "5");
}
