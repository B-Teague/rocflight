//! Record field defaults and optional fields — the last of phase 14.
//!
//! Verified against `roc` nightly-2026-09-03:
//!   * `name : Type ?? default` is a DEFAULTED field. Omitting it at construction
//!     substitutes the default, so it is always present and read with plain `.name`.
//!   * `name ?: Type` is an OPTIONAL field. It may genuinely be absent, so it is read
//!     with `.?name`, which yields `Ok(value)` or `Err(MissingField)`.
//!   * both are only allowed on a nominal's backing record.
//!
//! Correction to an earlier note: `.?` does NOT segfault the compiler in general. It
//! segfaults when MISUSED on an ordinary field of a plain record — which is not what it
//! is for. On a nominal's `?:` field it works correctly.

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

fn accepts(src: &str) -> bool {
    TypeChecker::new().synth(&build(src)).is_ok()
}

const CFG: &str = "Cfg := { host: Str, port: U16 ?? 8080 }\n";
const OPT: &str = "Cfg := { host: Str, timeout ?: U64 }\n";

// --- defaults -------------------------------------------------------------

#[test]
fn an_omitted_defaulted_field_gets_its_default() {
    assert_eq!(
        as_str(&format!("{}c = Cfg.{{ host: \"a\" }}\nU16.to_str(c.port)", CFG)),
        "8080"
    );
}

#[test]
fn a_supplied_value_wins_over_the_default() {
    assert_eq!(
        as_str(&format!("{}c = Cfg.{{ host: \"a\", port: 99 }}\nU16.to_str(c.port)", CFG)),
        "99"
    );
}

#[test]
fn a_defaulted_field_needs_no_unwrapping() {
    // It is always present, so it reads like any other field.
    assert_eq!(
        as_str(&format!("{}c = Cfg.{{ host: \"a\" }}\n\"${{c.host}}:${{U16.to_str(c.port)}}\"", CFG)),
        "a:8080"
    );
}

#[test]
fn the_default_is_part_of_the_constructed_record() {
    assert_eq!(
        as_str(&format!("{}Str.inspect(Cfg.{{ host: \"a\" }})", CFG)),
        "{ host: \"a\", port: 8080 }"
    );
}

#[test]
fn defaults_belong_to_their_own_nominal() {
    // Two declarations must not share a scratch list of defaults.
    let src = "A := { x: I64 ?? 1 }\nB := { y: I64 ?? 2 }\n";
    assert_eq!(as_str(&format!("{}Str.inspect(A.{{}})", src)), "{ x: 1 }");
    assert_eq!(as_str(&format!("{}Str.inspect(B.{{}})", src)), "{ y: 2 }");
}

#[test]
fn defaults_are_filled_inside_string_interpolation() {
    // Each `${...}` gets its own sub-parser, which has to inherit the defaults or the
    // omitted field is silently left out.
    assert_eq!(
        as_str(&format!("{}\"v=${{U16.to_str(Cfg.{{ host: \"a\" }}.port)}}\"", CFG)),
        "v=8080"
    );
}

// --- optional fields ------------------------------------------------------

#[test]
fn an_absent_optional_field_reads_as_missing() {
    assert_eq!(
        as_str(&format!("{}f = |c| match c.?timeout {{ Ok(t) => U64.to_str(t) Err(MissingField) => \"none\" }}\nf(Cfg.{{ host: \"a\" }})", OPT)),
        "none"
    );
}

#[test]
fn a_present_optional_field_reads_as_ok() {
    assert_eq!(
        as_str(&format!("{}f = |c| match c.?timeout {{ Ok(t) => U64.to_str(t) Err(MissingField) => \"none\" }}\nf(Cfg.{{ host: \"a\", timeout: 30 }})", OPT)),
        "30"
    );
}

#[test]
fn a_record_without_its_optional_field_still_type_checks() {
    // `?:` means the field may genuinely be absent, so record unification cannot
    // require it.
    assert!(accepts(&format!("{}c : Cfg\nc = Cfg.{{ host: \"a\" }}\nc", OPT)));
}

#[test]
fn a_record_with_its_optional_field_also_type_checks() {
    assert!(accepts(&format!("{}c : Cfg\nc = Cfg.{{ host: \"a\", timeout: 30 }}\nc", OPT)));
}

#[test]
fn a_missing_required_field_is_still_rejected() {
    // Relaxing unification for OPTIONAL fields must not relax it for required ones.
    assert!(
        !accepts("r : { a: I64, b: I64 }\nr = { a: 1 }\nr"),
        "a missing required field should fail"
    );
}

#[test]
fn an_unexpected_field_is_still_rejected() {
    assert!(
        !accepts("r : { a: I64 }\nr = { a: 1, extra: 2 }\nr"),
        "an unexpected field should fail"
    );
}

// --- defaults and optional fields are different ---------------------------

#[test]
fn a_defaulted_field_is_present_while_an_optional_one_may_not_be() {
    let both = "Cfg := { d: I64 ?? 7, o ?: I64 }\n";
    // The default is there...
    assert_eq!(as_str(&format!("{}Str.inspect(Cfg.{{}})", both)), "{ d: 7 }");
    // ...and the optional one is not, which `.?` reports.
    assert_eq!(
        as_str(&format!("{}f = |c| match c.?o {{ Ok(v) => I64.to_str(v) Err(MissingField) => \"absent\" }}\nf(Cfg.{{}})", both)),
        "absent"
    );
}
