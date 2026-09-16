//! Nominal method blocks — the rest of phase 14.
//!
//! Verified against `roc` nightly-2026-09-03:
//!   * `Name :: backing.{ ... }` holds ordinary functions, reached as `Name.method(...)`
//!   * `value.method(args)` is the same call with the receiver moved in front, exactly
//!     as static dispatch does for builtins
//!   * `::` is NOT opaque within one file: a raw record is accepted where the nominal
//!     is expected, just as with `:=`. Opacity needs module boundaries this interpreter
//!     does not have, so the two are treated alike.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Value;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn as_str(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    match rocflight::vm::eval(&ast).expect("eval failed") {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    }
}

fn eval_error(src: &str) -> String {
    let ast = build(src);
    let _ = TypeChecker::new().synth(&ast);
    rocflight::vm::eval(&ast).expect_err("expected an error").message
}

const COUNTER: &str = "Counter :: { n: I64 }.{\n    start : Counter\n    start = { n: 0 }\n\n    bump : Counter, I64 -> Counter\n    bump = |c, by| { ..c, n: c.n + by }\n\n    show : Counter -> Str\n    show = |c| c.n.to_str()\n}\n";

// --- the explicit form ----------------------------------------------------

#[test]
fn a_method_is_reached_as_type_dot_method() {
    assert_eq!(as_str(&format!("{}Counter.show(Counter.start)", COUNTER)), "0");
}

#[test]
fn a_zero_argument_method_is_a_plain_value() {
    // `start : Counter` has no parameters, so `Counter.start` is a value not a call.
    assert_eq!(
        as_str(&format!("{}Str.inspect(Counter.start)", COUNTER)),
        "{ n: 0 }"
    );
}

#[test]
fn methods_take_their_arguments_in_order() {
    assert_eq!(
        as_str(&format!("{}Counter.show(Counter.bump(Counter.start, 5))", COUNTER)),
        "5"
    );
}

// --- dispatch -------------------------------------------------------------

#[test]
fn a_method_may_be_dispatched_on_its_receiver() {
    assert_eq!(as_str(&format!("{}c = Counter.start\nc.show()", COUNTER)), "0");
}

#[test]
fn dispatch_and_the_explicit_form_agree() {
    let src = format!("{}c = Counter.bump(Counter.start, 5)\n", COUNTER);
    assert_eq!(as_str(&format!("{}c.show()", src)), as_str(&format!("{}Counter.show(c)", src)));
}

#[test]
fn dispatch_passes_extra_arguments_after_the_receiver() {
    // `c.bump(2)` is `Counter.bump(c, 2)`.
    assert_eq!(
        as_str(&format!("{}c = Counter.start\nc.bump(2).show()", COUNTER)),
        "2"
    );
}

#[test]
fn dispatch_chains_through_methods() {
    assert_eq!(
        as_str(&format!("{}Counter.start.bump(1).bump(2).show()", COUNTER)),
        "3"
    );
}

// --- a method body sees its parameter's type ------------------------------

#[test]
fn a_methods_annotation_gives_its_parameter_a_type() {
    // `show = |c| c.n.to_str()` only works because `show : Counter -> Str` is claimed
    // from inside the block — otherwise `c` is untyped and `c.n` cannot dispatch.
    assert_eq!(as_str(&format!("{}Counter.start.show()", COUNTER)), "0");
}

#[test]
fn field_access_reaches_through_a_nominal() {
    // `p.x` for `p : Point` reads the backing record's field.
    assert_eq!(
        as_str("Point := { x: I64 }\np : Point\np = Point.{ x: 7 }\nI64.to_str(p.x)"),
        "7"
    );
}

// --- ambiguity ------------------------------------------------------------

#[test]
fn a_method_name_two_types_share_is_reported_not_guessed() {
    // Values carry no nominal tag, so dispatch searches by method name. Two matches
    // cannot be told apart, and guessing would silently call the wrong one.
    let src = "A :: { v: I64 }.{\n    show : A -> Str\n    show = |a| a.v.to_str()\n}\n\nB :: { v: I64 }.{\n    show : B -> Str\n    show = |b| b.v.to_str()\n}\n\nx = { v: 1 }\nx.show()";
    let err = eval_error(src);
    assert!(err.contains("ambiguous"), "got {}", err);
    assert!(err.contains("A.show") && err.contains("B.show"), "it should name both: {}", err);
}

#[test]
fn the_explicit_form_is_never_ambiguous() {
    let src = "A :: { v: I64 }.{\n    show : A -> Str\n    show = |a| a.v.to_str()\n}\n\nB :: { v: I64 }.{\n    show : B -> Str\n    show = |b| b.v.to_str()\n}\n\nA.show({ v: 1 })";
    assert_eq!(as_str(src), "1");
}

// --- opacity --------------------------------------------------------------

#[test]
fn opaque_and_nominal_declarations_behave_alike() {
    // roc accepts a raw record where either is expected, within one file.
    assert_eq!(
        as_str("Secret :: { key: Str }.{\n    reveal : Secret -> Str\n    reveal = |s| s.key\n}\nSecret.reveal({ key: \"raw\" })"),
        "raw"
    );
}

#[test]
fn a_method_block_does_not_disturb_the_program_around_it() {
    // The methods are wrapped around the whole program, and only by the OUTERMOST
    // parse — wrapping at every level nested them inside the first method's own body.
    assert_eq!(
        as_str(&format!("{}other = \"kept\"\nother", COUNTER)),
        "kept"
    );
}
