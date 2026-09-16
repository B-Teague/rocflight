//! `match` — phase 11.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * arms are newline-separated; a comma between them is allowed but optional
//!   * roc REQUIRES the arms to be exhaustive
//!   * `_` is the wildcard; patterns nest to any depth; `A | B` shares one body
//!   * a guard runs after its pattern matches, with the pattern's bindings in scope
//!
//! List patterns (`[]`, `[x, ..]`, `[1, .. as tail]`) are not covered: they need
//! lists, which are a later phase.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Value;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn eval(src: &str) -> Value {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    rocflight::vm::eval(&ast).expect("eval failed")
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

fn eval_err(src: &str) -> String {
    let ast = build(src);
    let _ = TypeChecker::new().synth(&ast);
    rocflight::vm::eval(&ast)
        .expect_err("expected an eval error")
        .message
}

// --- patterns -------------------------------------------------------------

#[test]
fn tag_patterns_select_an_arm() {
    let m = |c: &str| format!("match {} {{ Red => \"r\" Green => \"g\" Blue => \"b\" }}", c);
    assert_eq!(as_str(&m("Red")), "r");
    assert_eq!(as_str(&m("Green")), "g");
    assert_eq!(as_str(&m("Blue")), "b");
}

#[test]
fn literal_patterns_match_values() {
    let m = |n: &str| format!("match {} {{ 1 => \"one\" 2 => \"two\" _ => \"many\" }}", n);
    assert_eq!(as_str(&m("1")), "one");
    assert_eq!(as_str(&m("2")), "two");
    assert_eq!(as_str(&m("9")), "many");

    let s = |v: &str| format!("match {} {{ \"a\" => \"A\" _ => \"?\" }}", v);
    assert_eq!(as_str(&s("\"a\"")), "A");
    assert_eq!(as_str(&s("\"z\"")), "?");
}

#[test]
fn wildcard_matches_anything_and_binds_nothing() {
    assert_eq!(as_str(r#"match Red { _ => "anything" }"#), "anything");
}

#[test]
fn underscore_prefixed_name_is_a_binding_not_a_wildcard() {
    // `_` alone is the wildcard; `_unused` is an ordinary binding that documents
    // being unused, so it still binds.
    assert_eq!(as_str(r#"match "v" { _unused => _unused }"#), "v");
}

#[test]
fn binding_pattern_captures_the_value() {
    assert_eq!(as_str(r#"match "captured" { x => x }"#), "captured");
}

#[test]
fn alternatives_share_one_body() {
    let m = |c: &str| format!("match {} {{ Red | Orange => \"warm\" Blue => \"cool\" }}", c);
    assert_eq!(as_str(&m("Red")), "warm");
    assert_eq!(as_str(&m("Orange")), "warm");
    assert_eq!(as_str(&m("Blue")), "cool");
}

#[test]
fn payload_patterns_bind_positionally() {
    assert_eq!(
        as_str(r#"match Foo(42, "label") { Foo(n, s) => "${s}=${I64.to_str(n)}" Bar => "bar" }"#),
        "label=42"
    );
}

#[test]
fn patterns_nest() {
    let m = |v: &str| format!("match {} {{ Wrap(Inner(s)) => s Wrap(Empty) => \"empty\" Bare => \"bare\" }}", v);
    assert_eq!(as_str(&m(r#"Wrap(Inner("deep"))"#)), "deep");
    assert_eq!(as_str(&m("Wrap(Empty)")), "empty");
    assert_eq!(as_str(&m("Bare")), "bare");
}

#[test]
fn payload_arity_must_match_the_pattern() {
    // `Foo(a)` must not match a two-payload `Foo`.
    assert!(
        eval_err(r#"match Foo(1, 2) { Foo(a) => "one" }"#).contains("No match arm"),
        "a one-arg pattern should not match a two-payload tag"
    );
}

// --- order and guards -----------------------------------------------------

#[test]
fn arms_are_tried_in_order() {
    // The wildcard is last, so the specific arm wins. Reversed, the wildcard would.
    assert_eq!(as_str(r#"match 1 { 1 => "specific" _ => "general" }"#), "specific");
    assert_eq!(as_str(r#"match 1 { _ => "general" 1 => "specific" }"#), "general");
}

#[test]
fn a_false_guard_falls_through_to_the_next_arm() {
    let m = |n: &str| format!("match {} {{ 0 => \"zero\" x if x < 0 => \"neg\" _ => \"pos\" }}", n);
    assert_eq!(as_str(&m("0")), "zero");
    assert_eq!(as_str(&m("0 - 5")), "neg");
    assert_eq!(as_str(&m("5")), "pos");
}

#[test]
fn a_guard_sees_the_patterns_bindings() {
    assert_eq!(as_str(r#"match 7 { x if x > 5 => "big" _ => "small" }"#), "big");
}

#[test]
fn a_non_bool_guard_is_an_error() {
    assert!(
        eval_err(r#"match 1 { x if x => "yes" _ => "no" }"#).contains("Bool"),
        "a non-Bool guard should say so"
    );
}

// --- scoping --------------------------------------------------------------

#[test]
fn arm_bindings_do_not_leak_out_of_the_match() {
    // `inner` exists only inside its arm.
    let src = "outer = \"kept\"\nfirst = match Wrap(\"x\") { Wrap(inner) => inner }\nouter";
    assert_eq!(as_str(src), "kept");
}

#[test]
fn a_partial_nested_match_leaves_no_bindings_behind() {
    // `Pair(a, Inner(b))` binds `a`, then fails on the second element. `a` must not
    // leak into the next arm, or the fallback would see a stale value.
    let src = r#"match Pair("first", Other) { Pair(a, Inner(b)) => b _ => "fallback" }"#;
    assert_eq!(as_str(src), "fallback");
}

// --- exhaustiveness -------------------------------------------------------

#[test]
fn no_matching_arm_is_a_runtime_error() {
    // roc rejects a non-exhaustive match at compile time. The interpreter cannot:
    // it never sees the type annotations, so it has no idea what the full union is.
    // It reports the failure at run time rather than returning something wrong.
    // ponytail: becomes a compile-time check once annotations reach the AST.
    assert!(
        eval_err(r#"match Blue { Red => "r" Green => "g" }"#).contains("No match arm"),
        "an unmatched value should be reported"
    );
}

// --- as an expression -----------------------------------------------------

#[test]
fn match_is_an_expression() {
    // It has a value, so it can sit anywhere an expression can.
    assert_eq!(
        as_str(r#"Str.inspect(match Red { Red => "r" _ => "o" })"#),
        "\"r\""
    );
}

#[test]
fn arm_bodies_may_be_blocks() {
    assert_eq!(
        as_str("match Red {\n    Red => {\n        label = \"block\"\n        label\n    }\n    _ => \"other\"\n}"),
        "block"
    );
}

#[test]
fn commas_between_arms_are_optional() {
    assert_eq!(as_str(r#"match Red { Red => "r", Green => "g" }"#), "r");
    assert_eq!(as_str(r#"match Red { Red => "r" Green => "g" }"#), "r");
}

#[test]
fn match_keyword_boundary_is_respected() {
    assert_eq!(as_str("matcher = \"name\"\nmatcher"), "name");
}
