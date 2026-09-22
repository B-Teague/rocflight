//! Nominal types — phase 14.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `Name := backing` is NOT opaque: roc accepts the plain backing value where the
//!     nominal is expected (`f({ x: 1 })` for `f : Point -> _`)
//!   * but two DIFFERENT nominals with identical backing do not interchange
//!   * the nominal name is erased in values — `Str.inspect` on one shows the bare
//!     backing record
//!   * a nominal over a tag union is still exhaustiveness-checked
//!
//! `::` opaque types are accepted as a SYNONYM for `:=`, because within one file roc
//! does not distinguish them — both allow field access and both accept the plain
//! backing — and opacity only matters across module boundaries. Method blocks, field
//! defaults and optional fields all landed later in this phase; the notes that used to
//! sit here calling them unimplemented, and `.?` a compiler segfault, were both wrong.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Value;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn type_of(src: &str) -> String {
    TypeChecker::new().synth(&build(src)).expect("type check failed").to_string()
}

fn accepts(src: &str) -> bool {
    TypeChecker::new().synth(&build(src)).is_ok()
}

fn rejection(src: &str) -> String {
    TypeChecker::new().synth(&build(src)).expect_err("expected an error").message
}

fn as_str(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    match rocflight::vm::eval(&ast).expect("eval failed") {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

// --- declaration and construction -----------------------------------------

#[test]
fn a_nominal_annotation_resolves_to_the_nominal() {
    assert_eq!(type_of("Point := { x: I64 }\np : Point\np = Point.{ x: 1 }\np"), "Point");
}

#[test]
fn construction_yields_the_backing_value() {
    // The nominal name is erased: `Str.inspect` shows the bare record, as roc does.
    assert_eq!(
        as_str("Point := { x: I64, y: I64 }\np = Point.{ x: 3, y: 4 }\nStr.inspect(p)"),
        "{ x: 3, y: 4 }"
    );
}

#[test]
fn fields_are_read_with_plain_dot() {
    assert_eq!(
        as_str("Point := { x: I64 }\np = Point.{ x: 7 }\nI64.to_str(p.x)"),
        "7"
    );
}

#[test]
fn a_nominal_union_tag_is_just_the_tag() {
    // `Animal.Dog(x)` builds the same value a bare `Dog(x)` would.
    assert_eq!(
        as_str("Animal := [Dog(Str)]\nStr.inspect(Animal.Dog(\"rex\"))"),
        "Dog(\"rex\")"
    );
}

// --- distinctness, which is the point -------------------------------------

#[test]
fn different_nominals_do_not_interchange() {
    let err = rejection(
        "A := { x: I64 }\nB := { x: I64 }\nf : A -> I64\nf = |p| p.x\nb : B\nb = B.{ x: 1 }\nf(b)",
    );
    assert!(err.contains("different nominal"), "got {}", err);
}

#[test]
fn the_same_nominal_interchanges() {
    assert!(accepts("A := { x: I64 }\nf : A -> I64\nf = |p| p.x\na : A\na = A.{ x: 1 }\nf(a)"));
}

#[test]
fn the_backing_type_is_accepted_where_a_nominal_is_expected() {
    // `:=` is nominal but not opaque. Verified against roc, which accepts this.
    assert!(accepts("Point := { x: I64 }\nf : Point -> I64\nf = |p| p.x\nf({ x: 1 })"));
}

// --- patterns -------------------------------------------------------------

#[test]
fn a_nominal_destructuring_pattern_binds_fields() {
    assert_eq!(
        as_str("Point := { x: I64, y: I64 }\nf = |Point.{ x, y }| I64.to_str(x + y)\nf(Point.{ x: 9, y: 1 })"),
        "10"
    );
}

#[test]
fn a_field_pattern_may_rename_or_match() {
    // `{ x }` is shorthand for `{ x: x }`; an explicit pattern also works.
    assert_eq!(
        as_str("P := { x: I64 }\nf = |p| match p { P.{ x: 0 } => \"zero\" P.{ x } => I64.to_str(x) }\nf(P.{ x: 0 })"),
        "zero"
    );
}

#[test]
fn nominal_union_patterns_match() {
    let f = |v: &str| format!(
        "Animal := [Dog(Str), Cat(Str)]\nf = |a| match a {{ Animal.Dog(n) => \"woof:${{n}}\" Animal.Cat(n) => \"meow:${{n}}\" }}\nf({})",
        v);
    assert_eq!(as_str(&f("Animal.Dog(\"rex\")")), "woof:rex");
    assert_eq!(as_str(&f("Animal.Cat(\"tom\")")), "meow:tom");
}

#[test]
fn lambda_parameters_may_be_patterns() {
    // Needed by `|Point.{ x }|`, and it generalises: a tuple pattern works too.
    assert_eq!(as_str("f = |(a, b)| I64.to_str(a + b)\nf((2, 3))"), "5");
}

// --- exhaustiveness reaches through the nominal ---------------------------

#[test]
fn a_nominal_union_is_exhaustiveness_checked() {
    // The union is the nominal's backing type, so the check applies. roc rejects the
    // same program with "This match expression doesn't cover all possible cases."
    let err = rejection(
        "Animal := [Dog(Str), Cat(Str)]\nf : Animal -> Str\nf = |a| match a { Animal.Dog(n) => n }\nf",
    );
    assert!(err.contains("Cat"), "error should name the missing tag, got {}", err);
}

// --- deferred forms still parse ------------------------------------------

#[test]
fn opaque_declarations_are_accepted_like_nominal_ones() {
    assert!(accepts("Secret :: { key: Str }\nf : Secret -> Str\nf = |s| s.key\nf"));
}

#[test]
fn a_method_block_is_skipped_not_rejected() {
    // Methods need static dispatch; the block is consumed so the rest of the file is
    // still usable.
    assert!(accepts(
        "Animal := [Dog(Str)].{\n    speak = |a| \"woof\"\n}\nf : Animal -> Str\nf = |a| match a { Animal.Dog(n) => n }\nf"
    ));
}

#[test]
fn nominals_are_visible_inside_string_interpolation() {
    // Each `${...}` gets its own sub-parser, which has to inherit the declarations or
    // `Animal.Dog(x)` parses as a qualified call instead of a tag.
    assert_eq!(
        as_str("Animal := [Dog(Str)]\nname = |a| match a { Animal.Dog(n) => n }\n\"got ${name(Animal.Dog(\"rex\"))}\""),
        "got rex"
    );
}
