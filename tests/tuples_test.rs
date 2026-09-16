//! Tuples — phase 10.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `(1)` is a parenthesised expression, NOT a one-tuple
//!   * `.0` is zero-based and works on any receiver, including a call result
//!   * `Str.inspect` renders `("Roc", 1)` and recurses
//!   * destructuring works both inside a block and at the top level

use rocflight::desugaring::Desugarer;
use rocflight::eval::Value;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn parse(src: &str) -> Result<rocflight::ast::Expr, rocflight::error::ParseError> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr()
}

fn eval(src: &str) -> Value {
    let ast = parse(src).expect("parse failed");
    TypeChecker::new().synth(&ast).expect("type check failed");
    rocflight::vm::eval(&ast).expect("eval failed")
}

fn type_error(src: &str) -> String {
    TypeChecker::new().synth(&parse(src).expect("parse failed")).expect_err("expected a type error").message
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

// --- literals -------------------------------------------------------------

#[test]
fn tuple_literals_hold_mixed_types() {
    assert_eq!(as_str(r#"Str.inspect(("Roc", 1))"#), r#"("Roc", 1)"#);
    assert_eq!(as_str("Str.inspect((1, 2, 3))"), "(1, 2, 3)");
    // A trailing comma is allowed.
    assert_eq!(as_str("Str.inspect((1, 2,))"), "(1, 2)");
}

#[test]
fn a_single_paren_is_grouping_not_a_one_tuple() {
    // roc has no one-tuple: `(1)` is just `1`.
    assert_eq!(as_str("I64.to_str((1))"), "1");
    assert_eq!(as_str("I64.to_str((2 + 3) * 4)"), "20");
}

#[test]
fn tuples_nest_and_inspect_recurses() {
    assert_eq!(as_str(r#"Str.inspect((1, ("a", 2)))"#), r#"(1, ("a", 2))"#);
    assert_eq!(as_str(r#"Str.inspect([(1, "a"), (2, "b")])"#), r#"[(1, "a"), (2, "b")]"#);
}

#[test]
fn tuple_equality_is_elementwise() {
    assert_eq!(as_str(r#"Str.inspect((1, "x") == (1, "x"))"#), "True");
    assert_eq!(as_str(r#"Str.inspect((1, "x") == (2, "x"))"#), "False");
}

// --- indexing -------------------------------------------------------------

#[test]
fn index_is_zero_based() {
    assert_eq!(as_str(r#"p = ("Roc", 1)
p.0"#), "Roc");
    assert_eq!(as_str(r#"p = ("Roc", 1)
I64.to_str(p.1)"#), "1");
}

#[test]
fn index_works_on_any_receiver() {
    // Postfix, like record field access.
    assert_eq!(as_str("I64.to_str((7, 8).1)"), "8");
    assert_eq!(as_str("mk = |n| (n, n + 1)\nI64.to_str(mk(5).1)"), "6");
}

#[test]
fn out_of_range_index_is_an_error() {
    // Caught by the type checker when the tuple's type is known here.
    let ast = parse("(1, 2).5").unwrap();
    assert!(
        TypeChecker::new().synth(&ast).is_err(),
        ".5 on a 2-tuple should not type check"
    );
}

#[test]
fn indexing_a_non_tuple_is_an_error() {
    let ast = parse("x = Red\nx.0").unwrap();
    let _ = TypeChecker::new().synth(&ast);
    assert!(rocflight::vm::eval(&ast).is_err(), "indexing a tag should fail");
}

// --- patterns -------------------------------------------------------------

#[test]
fn tuple_patterns_match_elementwise() {
    let f = |p: &str| format!(r#"f = |p| match p {{ (0, 0) => "origin" (x, 0) => "x-axis" _ => "other" }}
f({})"#, p);
    assert_eq!(as_str(&f("(0, 0)")), "origin");
    assert_eq!(as_str(&f("(3, 0)")), "x-axis");
    assert_eq!(as_str(&f("(1, 1)")), "other");
}

#[test]
fn tuple_patterns_require_matching_arity() {
    // A tuple pattern CONSTRAINS the scrutinee to that arity, so a 3-tuple cannot
    // reach it. roc agrees, from the other end: it infers `p : (a, b)` from the first
    // pattern and reports the wildcard as redundant.
    let err = type_error(r#"f = |p| match p { (a, b) => "two" _ => "other" }
f((1, 2, 3))"#);
    assert!(err.contains("unify"), "arity mismatch should be a type error, got {}", err);
}

// --- destructuring --------------------------------------------------------

#[test]
fn destructuring_inside_a_block() {
    assert_eq!(
        as_str(r#"f = |_| {
    (name, n) = ("Roc", 1)
    "${name}${I64.to_str(n)}"
}
f(0)"#),
        "Roc1"
    );
}

#[test]
fn destructuring_at_the_top_level() {
    // Must NOT be scoped: the continuation is the rest of the file.
    assert_eq!(as_str(r#"pair = ("Roc", 1)
(name, n) = pair
"${name}${I64.to_str(n)}""#), "Roc1");
}

#[test]
fn a_wildcard_element_binds_nothing() {
    assert_eq!(as_str(r#"pair = ("Roc", 1)
(_, n) = pair
I64.to_str(n)"#), "1");
}

#[test]
fn a_grouped_expression_statement_is_not_destructuring() {
    // A statement may legitimately start with `(`. The destructuring check has to
    // backtrack when no `=` follows.
    assert_eq!(as_str(r#"f = |_| {
    (1 + 2)
}
I64.to_str(f(0))"#), "3");
}

#[test]
fn refutable_top_level_pattern_is_rejected_clearly() {
    // roc allows `(1, b) = pair`; the interpreter does not, because a top-level
    // binding cannot be a match. The error should point at the block form.
    let err = parse("pair = (1, 2)\n(1, b) = pair\nb").expect_err("should be rejected");
    assert!(
        err.message.contains("match") || err.message.contains("top-level"),
        "error should explain the restriction, got {:?}",
        err.message
    );
}
