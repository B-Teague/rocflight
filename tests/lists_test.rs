//! Lists and their builtins — phase 15, plus the list patterns it unblocks in 11.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `List.len` returns U64 — mixing it with I64 match arms is a type error
//!   * argument order is `List.map(list, fn)` and `List.fold(list, initial, fn)`,
//!     with the accumulator as the callback's FIRST parameter
//!   * `Str.inspect` renders `[1, 2, 3]` and recurses: `[[1], [2, 3]]`
//!   * `..` may sit at the end, middle, or start of a list pattern

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
fn list_literals_and_empty() {
    assert_eq!(as_str("Str.inspect([1, 2, 3])"), "[1, 2, 3]");
    assert_eq!(as_str("Str.inspect([])"), "[]");
    // A trailing comma is allowed.
    assert_eq!(as_str("Str.inspect([1, 2,])"), "[1, 2]");
}

#[test]
fn inspect_recurses_into_nested_lists() {
    assert_eq!(as_str("Str.inspect([[1], [2, 3]])"), "[[1], [2, 3]]");
    // Strings inside a list keep their quotes.
    assert_eq!(as_str(r#"Str.inspect(["a", "b"])"#), r#"["a", "b"]"#);
}

#[test]
fn lists_hold_any_value_kind() {
    assert_eq!(as_str("Str.inspect([Red, Green])"), "[Red, Green]");
    assert_eq!(as_str("Str.inspect([{ b: 1, a: 2 }])"), "[{ a: 2, b: 1 }]");
}

#[test]
fn list_equality_is_elementwise_and_length_sensitive() {
    assert_eq!(as_str("Str.inspect([1, 2] == [1, 2])"), "True");
    assert_eq!(as_str("Str.inspect([1, 2] == [2, 1])"), "False");
    assert_eq!(as_str("Str.inspect([1] == [1, 2])"), "False");
    assert_eq!(as_str("Str.inspect([] == [])"), "True");
}

// --- builtins -------------------------------------------------------------

#[test]
fn list_len_and_is_empty() {
    assert_eq!(as_str("I64.to_str(List.len([1, 2, 3]))"), "3");
    assert_eq!(as_str("I64.to_str(List.len([]))"), "0");
    assert_eq!(as_str("Str.inspect(List.is_empty([]))"), "True");
    assert_eq!(as_str("Str.inspect(List.is_empty([1]))"), "False");
}

#[test]
fn list_map_takes_the_list_first() {
    assert_eq!(as_str("Str.inspect(List.map([1, 2, 3], |x| x * 2))"), "[2, 4, 6]");
    assert_eq!(as_str("Str.inspect(List.map([], |x| x * 2))"), "[]");
}

#[test]
fn list_fold_accumulates_with_acc_first() {
    assert_eq!(as_str("I64.to_str(List.fold([1, 2, 3, 4], 0, |acc, x| acc + x))"), "10");
    // The initial value is returned untouched for an empty list.
    assert_eq!(as_str("I64.to_str(List.fold([], 99, |acc, x| acc + x))"), "99");
    // Order matters: subtraction would differ if acc and x were swapped.
    assert_eq!(as_str("I64.to_str(List.fold([1, 2], 10, |acc, x| acc - x))"), "7");
}

#[test]
fn unknown_list_builtin_is_reported() {
    let ast = parse("List.nope([1])").unwrap();
    let err = Evaluator::new().eval(&ast).expect_err("should fail");
    assert!(err.message.contains("List.nope"), "got {}", err.message);
}

// --- list patterns --------------------------------------------------------

#[test]
fn exact_length_list_patterns() {
    let f = |xs: &str| format!(r#"f = |xs| match xs {{ [] => "none" [x] => "one" [a, b] => "two" _ => "many" }}
f({})"#, xs);
    assert_eq!(as_str(&f("[]")), "none");
    assert_eq!(as_str(&f("[1]")), "one");
    assert_eq!(as_str(&f("[1, 2]")), "two");
    assert_eq!(as_str(&f("[1, 2, 3]")), "many");
}

#[test]
fn list_patterns_bind_elements() {
    assert_eq!(
        as_str(r#"f = |xs| match xs { [a, b] => I64.to_str(a + b) _ => "no" }
f([3, 4])"#),
        "7"
    );
}

#[test]
fn literal_elements_must_be_equal() {
    let f = |xs: &str| format!(r#"f = |xs| match xs {{ [1, 2] => "onetwo" [a, b] => "other" _ => "no" }}
f({})"#, xs);
    assert_eq!(as_str(&f("[1, 2]")), "onetwo");
    assert_eq!(as_str(&f("[9, 9]")), "other");
}

#[test]
fn rest_pattern_at_the_end() {
    let f = |xs: &str| format!(r#"f = |xs| match xs {{ [1, 2, ..] => "yes" _ => "no" }}
f({})"#, xs);
    assert_eq!(as_str(&f("[1, 2, 3, 4]")), "yes");
    // `..` matches zero elements too.
    assert_eq!(as_str(&f("[1, 2]")), "yes");
    assert_eq!(as_str(&f("[1]")), "no");
}

#[test]
fn rest_pattern_in_the_middle() {
    let f = |xs: &str| format!(r#"f = |xs| match xs {{ [2, .., 1] => "yes" _ => "no" }}
f({})"#, xs);
    assert_eq!(as_str(&f("[2, 8, 8, 1]")), "yes");
    assert_eq!(as_str(&f("[2, 1]")), "yes");
    assert_eq!(as_str(&f("[2, 8, 8, 9]")), "no");
}

#[test]
fn rest_pattern_at_the_start() {
    let f = |xs: &str| format!(r#"f = |xs| match xs {{ [.., 5] => "yes" _ => "no" }}
f({})"#, xs);
    assert_eq!(as_str(&f("[1, 2, 5]")), "yes");
    assert_eq!(as_str(&f("[5]")), "yes");
    assert_eq!(as_str(&f("[5, 1]")), "no");
}

#[test]
fn rest_can_bind_the_skipped_elements() {
    assert_eq!(
        as_str(r#"f = |xs| match xs { [9, .. as tail] => Str.inspect(tail) _ => "no" }
f([9, 4, 5])"#),
        "[4, 5]"
    );
    // Binding an empty middle gives an empty list, not a failure.
    assert_eq!(
        as_str(r#"f = |xs| match xs { [9, .. as tail] => Str.inspect(tail) _ => "no" }
f([9])"#),
        "[]"
    );
}

#[test]
fn only_one_rest_per_pattern() {
    let err = parse(r#"f = |xs| match xs { [.., 1, ..] => "x" _ => "y" }
f([1])"#)
        .expect_err("two `..` should be rejected");
    assert!(err.message.contains(".."), "got {:?}", err.message);
}

#[test]
fn a_list_pattern_does_not_match_a_non_list() {
    // A list pattern CONSTRAINS the scrutinee, so a tag cannot reach it — roc rejects
    // this the same way: "This argument has the type [Red, ..] but f needs ...".
    let err = type_error(r#"f = |v| match v { [a] => "list" _ => "other" }
f(Red)"#);
    assert!(err.contains("List"), "error should mention List, got {}", err);
}

// --- the apply refactor ---------------------------------------------------

#[test]
fn lambdas_still_work_in_both_call_positions() {
    // Both `eval` call sites now share one `apply`. A named binding...
    assert_eq!(as_str("f = |x| x\nf(\"direct\")"), "direct");
    // ...and a chained call, which goes through the other site.
    assert_eq!(as_str("mk = |_| |x| x\nmk(0)(\"chained\")"), "chained");
}

#[test]
fn calling_a_non_function_reports_the_value() {
    let ast = parse("f = |x| x\nList.map([1], 5)").unwrap();
    let err = Evaluator::new().eval(&ast).expect_err("should fail");
    assert!(err.message.contains("non-function"), "got {}", err.message);
}
