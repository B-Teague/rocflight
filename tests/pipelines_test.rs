//! Pipelines — phase 13.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `x |> f` is `f(x)`; `x |> f(a)` is `f(x, a)` — the piped value is PREPENDED,
//!     the same convention static dispatch uses
//!   * left-associative: `x |> f |> g` is `g(f(x))`
//!   * it binds TIGHTER than every binary operator, which is the opposite of most
//!     languages: `1 + 2 |> inc` is `1 + inc(2)` = 4, and `2 * 3 |> inc` is
//!     `2 * inc(3)` = 8

use rocflight::desugaring::Desugarer;
use rocflight::eval::{Evaluator, Value};
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn as_str(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    match Evaluator::new().eval(&ast).expect("eval failed") {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

const DEFS: &str = "double : I64 -> I64\ndouble = |n| n * 2\ninc : I64 -> I64\ninc = |n| n + 1\nsub : I64, I64 -> I64\nsub = |a, b| a - b\n";

fn piped(expr: &str) -> String {
    as_str(&format!("{}({}).to_str()", DEFS, expr))
}

// --- the basic form -------------------------------------------------------

#[test]
fn a_pipe_applies_the_function_to_the_value() {
    assert_eq!(piped("21 |> double"), "42");
}

#[test]
fn the_piped_value_is_prepended_to_written_arguments() {
    // `10 |> sub(4)` is `sub(10, 4)` = 6, not `sub(4, 10)` = -6. Subtraction is the
    // test that catches a swap.
    assert_eq!(piped("10 |> sub(4)"), "6");
}

#[test]
fn pipes_are_left_associative() {
    // `g(f(x))`, so the written order is the reading order.
    assert_eq!(piped("5 |> double |> inc"), "11");
    // Reversed, the arithmetic differs — proving the order is real.
    assert_eq!(piped("5 |> inc |> double"), "12");
}

#[test]
fn a_lambda_may_be_the_target() {
    assert_eq!(piped("20 |> (|n| n + 1)"), "21");
}

#[test]
fn a_qualified_function_may_be_the_target() {
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2]\n(xs |> List.len()).to_str()"), "2");
}

// --- precedence, which is the surprising part -----------------------------

#[test]
fn a_pipe_binds_tighter_than_addition() {
    // `1 + inc(2)` = 4, NOT `inc(1 + 2)` = 4... which happens to collide, so use
    // numbers where the two readings differ.
    assert_eq!(piped("1 + 2 |> double"), "5"); // 1 + double(2), not double(3)=6
}

#[test]
fn a_pipe_binds_tighter_than_multiplication() {
    assert_eq!(piped("2 * 3 |> inc"), "8"); // 2 * inc(3), not inc(6)=7
}

#[test]
fn a_pipe_binds_tighter_than_integer_division() {
    assert_eq!(piped("6 // 2 |> inc"), "2"); // 6 // inc(2), not inc(3)=4
}

#[test]
fn a_pipe_binds_tighter_than_comparison() {
    assert_eq!(
        as_str(&format!("{}Str.inspect(4 == 2 |> double)", DEFS)),
        "True" // 4 == double(2), not double(4 == 2) which would not type check
    );
}

#[test]
fn parens_still_override_precedence() {
    assert_eq!(piped("(1 + 2) |> double"), "6");
}

// --- interaction with other `|` syntax ------------------------------------

#[test]
fn a_match_alternative_is_not_read_as_a_pipe() {
    // `A | B` in a pattern and `|>` in an expression both start with `|`.
    assert_eq!(
        as_str("f = |c| match c { Red | Green => \"rg\" Blue => \"b\" }\nf(Green)"),
        "rg"
    );
}

#[test]
fn a_lambda_boundary_is_not_read_as_a_pipe() {
    assert_eq!(piped("7 |> (|n| n * 3)"), "21");
}

#[test]
fn or_is_not_read_as_a_pipe() {
    assert_eq!(
        as_str("Str.inspect(Bool.True or Bool.False)"),
        "True"
    );
}

// --- composes with the rest ----------------------------------------------

#[test]
fn a_pipe_target_can_be_dispatched_on() {
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2, 3]\n(xs |> List.len()).to_str()"), "3");
}

#[test]
fn a_pipe_works_inside_interpolation() {
    assert_eq!(piped("(3 |> double)"), "6");
    assert_eq!(as_str(&format!("{}\"v={{}}\"", DEFS).replace("{}", "${(3 |> double).to_str()}")), "v=6");
}
