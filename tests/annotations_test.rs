//! Type annotations in the AST, and the three checks they unlock.
//!
//! Annotations used to be skipped by the parser and thrown away. That single gap was
//! the root cause of three separate documented ceilings:
//!
//!   1. identifiers had no type — every `Expr::Ident` synthesised a fresh variable
//!   2. a closed tag union could not be enforced — `c : [Red, Green]` accepted `Blue`
//!   3. `match` exhaustiveness could not be checked — nothing knew the full union
//!
//! Verified against `roc` nightly-2026-09-03: a GUARDED arm does not count towards
//! coverage, which roc also rejects with "This match expression doesn't cover all
//! possible cases."

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

/// The type as the program will see it, with an unpinned numeral defaulted.
fn defaulted_type_of(src: &str) -> String {
    let mut checker = TypeChecker::new();
    let ty = checker.synth(&build(src)).expect("type check failed");
    checker.defaulted(&ty).to_string()
}

fn type_of(src: &str) -> String {
    TypeChecker::new().synth(&build(src)).expect("type check failed").to_string()
}

fn rejection(src: &str) -> String {
    TypeChecker::new()
        .synth(&build(src))
        .expect_err("expected a type error")
        .message
}

fn accepts(src: &str) -> bool {
    TypeChecker::new().synth(&build(src)).is_ok()
}

// --- the annotation reaches the AST at all ---------------------------------

#[test]
fn an_annotation_gives_the_binding_its_declared_type() {
    assert_eq!(type_of("x : U8\nx = 255\nx"), "U8");
    assert_eq!(type_of("s : Str\ns = \"hi\"\ns"), "Str");
    assert_eq!(type_of("xs : List(I64)\nxs = [1]\nxs"), "List(I64)");
}

#[test]
fn annotations_survive_desugaring() {
    let desugared = Desugarer::new("x : U8\nx = 255\n".to_string()).desugar().unwrap();
    assert!(desugared.contains("x : U8"), "annotation was stripped");
}

#[test]
fn every_annotation_form_in_the_corpus_parses() {
    // Records, tuples, tag unions, functions, applied types, and `Try`.
    for src in [
        "x : { a: Bool, b: I64 }\nx = { a: Bool.True, b: 1 }\nx",
        "x : (Str, I64)\nx = (\"a\", 1)\nx",
        "x : [Foo(I64, Str), Bar]\nx = Bar\nx",
        "f : I64, I64 -> I64\nf = |a, b| a + b\nf",
        "f : (I64, I64) -> I64\nf = |p| p.0\nf",
        "x : List(List(I64))\nx = [[1]]\nx",
        "x : {}\nx = {}\nx",
    ] {
        assert!(accepts(src), "should type check: {}", src);
    }
}

#[test]
fn try_is_the_ok_err_union() {
    // `Try(a, b)` IS `[Ok(a), Err(b)]` in roc, so the annotation is checkable.
    assert_eq!(
        type_of("x : Try(I64, Str)\nx = Ok(1)\nx"),
        "[Err(Str), Ok(I64)]"
    );
}

#[test]
fn a_multi_param_annotation_curries() {
    assert_eq!(type_of("f : Str, I64 -> Bool\nf = |a, b| Bool.True\nf"), "(Str -> (I64 -> Bool))");
    // Parenthesised, it is ONE tuple parameter instead of two.
    assert_eq!(type_of("f : (Str, I64) -> Bool\nf = |p| Bool.True\nf"), "((Str, I64) -> Bool)");
}

// --- feature 1: identifiers carry a type ----------------------------------

#[test]
fn a_named_non_function_cannot_be_called() {
    // Previously accepted: `x` synthesised a fresh var, which unified with anything.
    assert!(!accepts("x = 42\nx(1)"), "calling an I64 should fail");
}

#[test]
fn inference_flows_through_names_without_annotations() {
    // An unconstrained numeral has no width until something gives it one, and roc
    // then DEFAULTS it — `x = 42` prints `42.0`, a `Dec`. Nothing here constrains it,
    // so what comes back is the default rather than the old unconditional I64.
    assert_eq!(defaulted_type_of("x = 42\nx"), "Dec");
    assert_eq!(type_of("n : I64\nn = 42\nn"), "I64");
    assert_eq!(type_of("r = { a: Bool.True }\nr.a"), "Bool");
}

// --- feature 2: closed tag unions -----------------------------------------

#[test]
fn a_closed_union_rejects_a_tag_it_does_not_list() {
    let err = rejection("c : [Red, Green]\nc = Blue\nc");
    assert!(err.contains("Blue"), "error should name the tag, got {}", err);
}

#[test]
fn a_closed_union_accepts_its_own_tags() {
    assert!(accepts("c : [Red, Green]\nc = Green\nc"));
}

#[test]
fn an_inferred_union_stays_open() {
    // A tag expression means "at least this tag", so unification may add more. Only an
    // annotation closes a union.
    assert_eq!(type_of("Red"), "[Red, ..]");
    assert!(accepts("c : [Red, Green, ..]\nc = Blue\nc"));
}

#[test]
fn annotations_also_catch_non_tag_mismatches() {
    // All of these were invisible before, for the same reason.
    assert!(!accepts("r : { x: Bool }\nr = { x: 1 }\nr"), "record field type");
    assert!(!accepts("xs : List(I64)\nxs = [\"a\"]\nxs"), "list element type");
    assert!(!accepts("t : (I64, I64)\nt = (1, 2, 3)\nt"), "tuple arity");
    assert!(!accepts("v : [Foo(I64)]\nv = Foo(\"s\")\nv"), "payload type");
}

// --- feature 3: match exhaustiveness --------------------------------------

#[test]
fn a_match_missing_a_case_is_rejected() {
    let err = rejection(
        "f : [Red, Green, Blue] -> Str\nf = |c| match c { Red => \"r\" Green => \"g\" }\nf",
    );
    assert!(err.contains("Blue"), "error should name the missing tag, got {}", err);
}

#[test]
fn full_coverage_is_accepted() {
    assert!(accepts(
        "f : [Red, Green, Blue] -> Str\nf = |c| match c { Red => \"r\" Green => \"g\" Blue => \"b\" }\nf"
    ));
}

#[test]
fn a_wildcard_or_binding_completes_a_match() {
    assert!(accepts("f : [Red, Green, Blue] -> Str\nf = |c| match c { Red => \"r\" _ => \"o\" }\nf"));
    assert!(accepts("f : [Red, Green, Blue] -> Str\nf = |c| match c { Red => \"r\" x => \"o\" }\nf"));
}

#[test]
fn alternatives_count_towards_coverage() {
    assert!(accepts(
        "f : [Red, Green, Blue] -> Str\nf = |c| match c { Red | Green => \"rg\" Blue => \"b\" }\nf"
    ));
}

#[test]
fn a_guarded_arm_does_not_count_as_coverage() {
    // It may not run, so it cannot complete a match. roc agrees: the same program is
    // rejected with "This match expression doesn't cover all possible cases."
    let err = rejection(
        "f : [Red, Green], I64 -> Str\nf = |c, n| match c { Red => \"r\" _ if n > 0 => \"p\" }\nf",
    );
    assert!(err.contains("Green"), "error should name the missing tag, got {}", err);
}

#[test]
fn an_open_or_unannotated_scrutinee_is_not_checked() {
    // There is no list of "all the cases" to check against, so nothing is claimed.
    assert!(accepts("f : [Red, Green, ..] -> Str\nf = |c| match c { Red => \"r\" _ => \"o\" }\nf"));
    assert!(accepts("f = |c| match c { Red => \"r\" }\nf"));
}

// --- pattern bindings get real types --------------------------------------

#[test]
fn payload_bindings_take_their_declared_types() {
    assert!(accepts(
        "f : [Foo(I64, Str)] -> Str\nf = |t| match t { Foo(n, s) => \"${s}${I64.to_str(n)}\" }\nf"
    ));
    // ...and a wrong use of one is caught.
    assert!(
        !accepts("f : [Foo(I64)] -> Str\nf = |t| match t { Foo(n) => n }\nf"),
        "an I64 payload should not satisfy a Str result"
    );
}

#[test]
fn a_rest_binding_is_a_list_of_the_element_type() {
    assert!(accepts(
        "f : List(I64) -> I64\nf = |xs| match xs { [9, .. as tail] => List.len(tail) _ => 0 }\nf"
    ));
}
