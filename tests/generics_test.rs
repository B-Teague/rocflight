//! Type variables and generics — phase 18.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `identity : a -> a` may be used at two DIFFERENT types in one program
//!   * within one signature a repeated name is ONE variable: `pair : a, a -> a`
//!     rejects `pair(1, "s")`
//!   * across signatures the names are unrelated
//!   * `List(a) -> List(a)` ties the element types of argument and result together

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

/// The type as the program will see it. An unconstrained NUMERAL has no width until
/// something gives it one, and roc then defaults it to `Dec` — `identity(5)` is a Dec
/// for the same reason `x = 5` prints `5.0`.
fn type_of(src: &str) -> String {
    let mut checker = TypeChecker::new();
    let ty = checker.synth(&build(src)).expect("type check failed");
    checker.defaulted(&ty).to_string()
}

fn accepts(src: &str) -> bool {
    TypeChecker::new().synth(&build(src)).is_ok()
}

const ID: &str = "identity : a -> a\nidentity = |x| x\n";
const PAIR: &str = "pair : a, a -> a\npair = |x, _y| x\n";
const FIRST: &str = "first : a, b -> a\nfirst = |x, _y| x\n";

// --- generalisation -------------------------------------------------------

#[test]
fn a_generic_function_works_at_one_type() {
    assert_eq!(type_of(&format!("{}identity(\"hi\")", ID)), "Str");
    assert_eq!(type_of(&format!("{}identity(5)", ID)), "Dec");
}

#[test]
fn a_generic_function_works_at_two_types_in_one_program() {
    // The whole point. Without instantiating per use, the first call pins `a` and the
    // second fails with "Cannot unify Str with I64".
    assert!(accepts(&format!("{}s = identity(\"hi\")\nn = identity(5)\nn", ID)));
}

#[test]
fn each_use_gets_its_own_instance() {
    assert_eq!(
        type_of(&format!("{}s = identity(\"hi\")\nn = identity(5)\nn", ID)),
        "Dec"
    );
}

#[test]
fn a_generic_function_survives_three_uses() {
    assert!(accepts(&format!(
        "{}a = identity(\"s\")\nb = identity(1)\nc = identity(Bool.True)\nc",
        ID
    )));
}

// --- repeated variables ---------------------------------------------------

#[test]
fn a_repeated_variable_ties_positions_together() {
    assert!(accepts(&format!("{}pair(1, 2)", PAIR)));
}

#[test]
fn a_repeated_variable_rejects_mismatched_types() {
    // `pair : a, a -> a` means one type for both. This was wrongly accepted while each
    // occurrence of `a` became a separate fresh variable.
    assert!(
        !accepts(&format!("{}pair(1, \"s\")", PAIR)),
        "a, a -> a should reject mixed argument types"
    );
}

#[test]
fn distinct_variables_may_differ() {
    assert!(accepts(&format!("{}first(1, \"s\")", FIRST)));
    assert_eq!(type_of(&format!("{}first(1, \"s\")", FIRST)), "Dec");
}

#[test]
fn variables_are_scoped_to_their_own_signature() {
    // The `a` in `pair` is unrelated to the `a` in `first`.
    assert!(accepts(&format!("{}{}x = pair(1, 2)\ny = first(\"s\", 1)\ny", PAIR, FIRST)));
}

// --- generic containers ---------------------------------------------------

#[test]
fn a_variable_inside_a_container_is_still_generic() {
    let src = "echo_list : List(a) -> List(a)\necho_list = |xs| xs\n";
    assert_eq!(type_of(&format!("{}echo_list([1])", src)), "List(Dec)");
    assert!(accepts(&format!("{}n = echo_list([1])\ns = echo_list([\"a\"])\ns", src)));
}

#[test]
fn argument_and_result_element_types_are_tied() {
    let src = "echo_list : List(a) -> List(a)\necho_list = |xs| xs\n";
    // The result is a list of the SAME element type, not an independent one.
    assert_eq!(type_of(&format!("{}echo_list([\"a\"])", src)), "List(Str)");
}

#[test]
fn a_generic_argument_with_a_concrete_result() {
    let src = "count : List(a) -> U64\ncount = |xs| List.len(xs)\n";
    assert!(accepts(&format!("{}n = count([1])\ns = count([\"a\"])\ns", src)));
}

// --- the id-space bug -----------------------------------------------------

#[test]
fn annotation_variables_do_not_collide_with_inferred_ones() {
    // The parser and the checker allocate type-variable ids from the same number
    // space, so an annotation's `$1` would otherwise BE the checker's first fresh
    // variable and unify with something unrelated. Instantiating on ingest keeps the
    // parser's ids out of unification entirely.
    //
    // This showed up as `List(a) -> List(a)` failing with "Cannot unify List(I64)
    // with I64" — two unrelated types meeting through a shared id.
    let src = "echo_list : List(a) -> List(a)\necho_list = |xs| xs\n";
    assert_eq!(type_of(&format!("{}echo_list([1])", src)), "List(Dec)");
}

#[test]
fn a_monomorphic_binding_is_not_generalised() {
    // Only annotation variables are quantified. An inferred type stays put, so a
    // plain binding cannot be used at two types.
    assert!(!accepts("x = 42\ns : Str\ns = x\ns"), "an I64 binding is not generic");
}
