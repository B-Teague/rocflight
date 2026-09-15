//! Record literals, Bool, field access, and the operators they unblocked.
//!
//! Behaviour here was verified against `roc` nightly-2026-09-03 before being
//! implemented, not inferred.

use rocflight::desugaring::Desugarer;
use rocflight::eval::{Evaluator, Value};
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn eval(src: &str) -> Value {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().expect("parse failed");
    TypeChecker::new().synth(&ast).expect("type check failed");
    Evaluator::new().eval(&ast).expect("eval failed")
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

fn as_bool(src: &str) -> bool {
    match eval(src) {
        Value::Bool(b) => b,
        other => panic!("expected Bool, got {:?}", other),
    }
}

// --- record literals -------------------------------------------------------

#[test]
fn record_literal_keeps_source_order() {
    match eval("{ zebra: 1, apple: 2 }") {
        Value::Record(fields) => {
            let names: Vec<&str> = fields.iter().map(|(n, _)| *n).collect();
            // Only Str.inspect sorts; the value itself keeps source order.
            assert_eq!(names, vec!["zebra", "apple"]);
        }
        other => panic!("expected Record, got {:?}", other),
    }
}

#[test]
fn record_allows_trailing_comma() {
    match eval("{ x: 1, y: 2, }") {
        Value::Record(fields) => assert_eq!(fields.len(), 2),
        other => panic!("expected Record, got {:?}", other),
    }
}

#[test]
fn record_field_values_are_full_expressions() {
    assert_eq!(as_str("r = { sum: 2 + 3 * 4 }\nI64.to_str(r.sum)"), "14");
}

#[test]
fn empty_braces_are_unit_not_a_record() {
    assert!(matches!(eval("{}"), Value::Unit));
}

// --- record vs block disambiguation ---------------------------------------

#[test]
fn braces_with_annotated_binding_are_a_block_not_a_record() {
    // `n : I64` has a space before the colon, so it is an annotation inside a
    // block. A record field is `n: value`, with no space. Conflating the two would
    // silently turn a block into a one-field record.
    assert_eq!(as_str("f = |_| {\n    n : I64\n    n = 21\n    I64.to_str(n * 2)\n}\nf(0)"), "42");
}

#[test]
fn braces_with_a_field_are_a_record_not_a_block() {
    match eval("{ n: 21 }") {
        Value::Record(fields) => assert_eq!(fields.len(), 1),
        other => panic!("expected Record, got {:?}", other),
    }
}

// --- field access ---------------------------------------------------------

#[test]
fn field_access_chains() {
    assert_eq!(as_str("r = { a: { b: 7 } }\nI64.to_str(r.a.b)"), "7");
}

#[test]
fn uppercase_receiver_is_a_module_not_a_field() {
    // `Str.inspect` must stay a module member; only lowercase receivers are fields.
    assert_eq!(as_str("Str.inspect(Bool.True)"), "True");
}

#[test]
fn missing_field_is_an_error() {
    // Caught at eval. The type checker cannot catch it here: the receiver is an
    // identifier, which synths to a fresh type variable because there is still no
    // type environment, so the record's field list is not visible.
    // ponytail: moves to the type checker once `synth` has an environment.
    let desugared = Desugarer::new("r = { a: 1 }\nr.nope".to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().unwrap();
    assert!(Evaluator::new().eval(&ast).is_err(), "missing field should fail at eval");
}

#[test]
fn missing_field_on_a_literal_is_caught_by_the_type_checker() {
    // When the receiver is the literal itself, its type IS known, so the checker
    // rejects it before evaluation.
    let desugared = Desugarer::new("{ a: 1 }.nope".to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().unwrap();
    assert!(TypeChecker::new().synth(&ast).is_err(), "missing field should fail");
}

// --- Str.inspect ----------------------------------------------------------

#[test]
fn inspect_sorts_record_fields_alphabetically() {
    // Verified against roc: `{ zebra: .., apple: .., mango: .. }` inspects sorted.
    assert_eq!(
        as_str("Str.inspect({ zebra: Bool.True, apple: Bool.False, mango: Bool.True })"),
        "{ apple: False, mango: True, zebra: True }"
    );
}

#[test]
fn inspect_sorts_nested_records_too() {
    assert_eq!(
        as_str("Str.inspect({ outer: Bool.True, inner: { y: Bool.False, x: Bool.True } })"),
        "{ inner: { x: True, y: False }, outer: True }"
    );
}

#[test]
fn inspect_quotes_strings_and_names_bools() {
    assert_eq!(as_str("Str.inspect(\"hi\")"), "\"hi\"");
    assert_eq!(as_str("Str.inspect(Bool.False)"), "False");
    assert_eq!(as_str("Str.inspect({})"), "{}");
}

// --- operators unblocked by the above -------------------------------------

#[test]
fn comparisons_yield_bool() {
    assert!(as_bool("1 < 2"));
    assert!(!as_bool("2 < 1"));
    assert!(as_bool("2 <= 2"));
    assert!(as_bool("\"a\" == \"a\""));
}

#[test]
fn and_or_keywords_work_like_the_symbols() {
    assert!(!as_bool("Bool.True and Bool.False"));
    assert!(as_bool("Bool.True or Bool.False"));
}

#[test]
fn keyword_boundary_is_respected() {
    // Without a boundary check, `android` would parse as `and` + `roid`.
    assert_eq!(as_str("android = \"phone\"\nandroid"), "phone");
    assert_eq!(as_str("organ = \"pipe\"\norgan"), "pipe");
}

#[test]
fn prefix_bang_is_logical_not() {
    // Unrelated to the `!` that ends an effectful name.
    assert!(!as_bool("!Bool.True"));
    assert!(as_bool("Bool.not(Bool.False)"));
}

#[test]
fn int_div_and_rem() {
    assert_eq!(as_str("I64.to_str(7 // 2)"), "3");
    assert_eq!(as_str("I64.to_str(7 % 2)"), "1");
    // `//` must not be read as two `/` operators.
    assert_eq!(as_str("I64.to_str(100 // 10 // 2)"), "5");
}

#[test]
fn int_div_by_zero_is_an_error() {
    let desugared = Desugarer::new("7 // 0".to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().unwrap();
    assert!(Evaluator::new().eval(&ast).is_err(), "division by zero should fail");
}

#[test]
fn field_access_works_on_any_receiver() {
    // Field access is postfix, so the receiver can be a literal or a call result,
    // not only a bare identifier.
    assert_eq!(as_str("I64.to_str({ a: 7 }.a)"), "7");
    assert_eq!(as_str("mk = |n| { v: n }\nI64.to_str(mk(7).v)"), "7");
}
