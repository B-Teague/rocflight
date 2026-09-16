//! Static dispatch — phase 20.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `receiver.method(args)` resolves through the receiver's TYPE, and the receiver
//!     becomes the FIRST argument — which is why roc's builtins take their subject
//!     first (`List.map(list, fn)`)
//!   * dispatching on an unresolved type is an error in roc too: "trying to dispatch a
//!     method named to_str on an unresolved type variable"
//!   * `s.is_empty` is a field read; `s.is_empty()` is a method call. The parens are
//!     the only difference.

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

fn type_error(src: &str) -> String {
    TypeChecker::new().synth(&build(src)).expect_err("expected a type error").message
}

fn eval_error(src: &str) -> String {
    let ast = build(src);
    let _ = TypeChecker::new().synth(&ast);
    Evaluator::new().eval(&ast).expect_err("expected an eval error").message
}

// --- the basic form -------------------------------------------------------

#[test]
fn a_method_call_resolves_through_the_receivers_type() {
    assert_eq!(as_str("n : I64\nn = 42\nn.to_str()"), "42");
    assert_eq!(as_str("s : Str\ns = \"hi\"\nStr.inspect(s.is_empty())"), "False");
}

#[test]
fn any_numeric_width_dispatches_to_the_same_place() {
    // The interpreter keeps one integer representation, so `U8` and `I64` land in the
    // same builtin.
    assert_eq!(as_str("small : U8\nsmall = 7\nsmall.to_str()"), "7");
}

#[test]
fn the_receiver_becomes_the_first_argument() {
    // `xs.fold(0, f)` is `List.fold(xs, 0, f)`. Subtraction catches an argument swap.
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2]\nxs.fold(10, |a, x| a - x).to_str()"), "7");
}

#[test]
fn extra_arguments_follow_the_receiver() {
    assert_eq!(
        as_str("xs : List(I64)\nxs = [1, 2, 3]\nStr.inspect(xs.map(|x| x * 2))"),
        "[2, 4, 6]"
    );
}

#[test]
fn a_zero_argument_method_still_needs_parens() {
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2, 3]\nxs.len().to_str()"), "3");
}

// --- chaining -------------------------------------------------------------

#[test]
fn dispatch_chains() {
    // The result of one dispatch is itself a receiver, so its type has to be known.
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2, 3]\nxs.len().to_str()"), "3");
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2]\nxs.fold(0, |a, x| a + x).to_str()"), "3");
    assert_eq!(as_str("xs : List(I64)\nxs = [1, 2]\nxs.map(|x| x).len().to_str()"), "2");
}

#[test]
fn a_method_works_on_any_receiver_expression() {
    // Postfix, like field access: a call result or a field is a fine receiver.
    assert_eq!(as_str("mk = |n| [n]\nmk(1).len().to_str()"), "1");
    assert_eq!(as_str("r : { s: Str }\nr = { s: \"\" }\nStr.inspect(r.s.is_empty())"), "True");
}

// --- field versus method --------------------------------------------------

#[test]
fn parens_separate_a_method_call_from_a_field_read() {
    // `.f` reads a field; `.f()` calls a method. Only the parens differ.
    assert_eq!(as_str("r = { f: 1 }\nI64.to_str(r.f)"), "1");
    assert_eq!(as_str("xs : List(I64)\nxs = [1]\nxs.len().to_str()"), "1");
}

// --- errors ---------------------------------------------------------------

#[test]
fn dispatching_on_an_unresolved_type_is_rejected() {
    // roc rejects this too. There is nothing to resolve the module from.
    let err = type_error("x = []\nx.first().to_str()");
    assert!(
        err.contains("unresolved"),
        "error should explain the receiver is unresolved, got {}",
        err
    );
}

#[test]
fn dispatch_inside_an_unannotated_lambda_is_deferred() {
    // A known ceiling. roc infers a lambda's parameter types from its CALL SITES; this
    // checker synthesises the body once, before any call site is seen, so a dispatch on
    // a parameter has nothing to resolve yet and falls back to the builtin table
    // instead of refusing. The call still evaluates correctly.
    assert_eq!(as_str("show = |x| x.to_str()\nshow(7)"), "7");
}

#[test]
fn an_unknown_method_names_the_module_it_looked_in() {
    let err = eval_error("n : I64\nn = 1\nn.nope()");
    assert!(err.contains("I64.nope"), "got {}", err);
}

#[test]
fn a_record_receiver_has_no_module_to_dispatch_on() {
    // Values carry no nominal wrapper, so there is nothing to look a method up in.
    let err = eval_error("r : { x: I64 }\nr = { x: 1 }\nr.nope()");
    assert!(err.contains("Cannot dispatch"), "got {}", err);
}

// --- the hole this work exposed -------------------------------------------

#[test]
fn expressions_inside_interpolation_are_type_checked() {
    // `StrInterp` used to synthesise as Str WITHOUT checking its parts, so any error
    // inside `${...}` went unreported — which is what hid broken chained dispatch in
    // this project's own golden pair.
    let err = type_error(r#""v=${1 + "s"}""#);
    assert!(err.contains("Cannot unify"), "got {}", err);
}
