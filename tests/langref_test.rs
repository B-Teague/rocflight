//! Phase 21 — the features the langref documents that earlier phases missed.
//!
//! Each has a golden pair under `tests/roc/21_langref/`; these pin the pieces that a
//! pair exercises only indirectly, and the ones where roc's behaviour is surprising
//! enough to be worth stating outright.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Evaluator;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

/// Parse, check and run, wiring `where` constraints from the parser into the checker
/// the way `main` does — the promise is discovered while parsing and has to be handed
/// over before checking begins.
fn value(src: &str) -> String {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().expect("parse failed");

    let mut checker = TypeChecker::new();
    checker.allow_dispatch(parser.where_methods());
    checker.synth(&ast).expect("type check failed");

    Evaluator::new().eval(&ast).expect("eval failed").to_string()
}

// --- grapheme literals ----------------------------------------------------

#[test]
fn a_grapheme_literal_is_a_number() {
    assert_eq!(value("'a'"), "97");
}

#[test]
fn a_grapheme_literal_takes_part_in_arithmetic() {
    assert_eq!(value("'a' + 1"), "98");
}

#[test]
fn a_grapheme_literal_holds_a_code_point_not_a_byte() {
    // 'é' is two bytes in UTF-8 but ONE code point, and the code point is the value.
    assert_eq!(value("'é'"), "233");
}

#[test]
fn a_grapheme_literal_understands_escapes() {
    assert_eq!(value("'\\n'"), "10");
    assert_eq!(value("'\\u(e9)'"), "233");
}

// --- ranges ---------------------------------------------------------------

#[test]
fn a_range_is_opaque_not_a_list() {
    // roc renders a range as `<opaque>`. Building one as a list would show
    // `[0, 1, 2]` here and would wrongly satisfy a `List` parameter.
    assert_eq!(value("0..<3"), "<opaque>");
}

#[test]
fn an_exclusive_range_stops_before_its_end() {
    assert_eq!(
        value("f = |r| {\n    var total = 0\n    for n in r {\n        total = total + n\n    }\n    total\n}\nf(0..<5)"),
        "10"
    );
}

#[test]
fn an_inclusive_range_reaches_its_end() {
    assert_eq!(
        value("f = |r| {\n    var total = 0\n    for n in r {\n        total = total + n\n    }\n    total\n}\nf(1..=5)"),
        "15"
    );
}

#[test]
fn a_range_binds_looser_than_arithmetic() {
    // `1 + 1..<2 * 3` is `2..<6`, not `1 + (1..<2) * 3`.
    assert_eq!(
        value("f = |r| {\n    var total = 0\n    for n in r {\n        total = total + n\n    }\n    total\n}\nf(1 + 1..<2 * 3)"),
        "14"
    );
}

// --- nominal construction over a non-record payload -----------------------

#[test]
fn a_nominal_over_a_single_value_is_its_payload() {
    assert_eq!(value("UserId := U64\nUserId.(7)"), "7");
}

#[test]
fn a_nominal_over_several_values_is_a_tuple() {
    assert_eq!(value("Pair := (I64, Str)\nPair.(1, \"two\")"), "(1, \"two\")");
}

// --- operators dispatch to methods ----------------------------------------

#[test]
fn a_type_that_defines_plus_gets_the_plus_operator() {
    let src = "Money :: { cents: I64 }.{\n    plus : Money, Money -> Money\n    plus = |a, b| { cents: a.cents + b.cents }\n}\n\
               a : Money\na = Money.{ cents: 5 }\nb : Money\nb = Money.{ cents: 7 }\n(a + b).cents";
    assert_eq!(value(src), "12");
}

#[test]
fn a_user_defined_is_eq_decides_equality_both_ways() {
    // `!=` asks for `is_eq` too, then negates it — roc names no separate method.
    let src = "Money :: { cents: I64 }.{\n    is_eq : Money, Money -> Bool\n    is_eq = |a, b| a.cents == b.cents\n}\n\
               a : Money\na = Money.{ cents: 5 }\nb : Money\nb = Money.{ cents: 9 }\n[a == b, a != b]";
    assert_eq!(value(src), "[False, True]");
}

#[test]
fn a_user_defined_operator_does_not_capture_the_primitives() {
    // A type defining `plus` must not hijack `1 + 2`.
    let src = "Money :: { cents: I64 }.{\n    plus : Money, Money -> Money\n    plus = |a, b| { cents: a.cents + b.cents }\n}\n1 + 2";
    assert_eq!(value(src), "3");
}

// --- type-level features --------------------------------------------------

#[test]
fn a_type_alias_is_transparent() {
    // `Bytes` and `List(U8)` are the same type, so a plain list passes for `Bytes`.
    let src = "Bytes : List(U8)\nsize : Bytes -> U64\nsize = |b| b.len()\nsize([1, 2, 3])";
    assert_eq!(value(src), "3");
}

#[test]
fn an_open_record_accepts_extra_fields() {
    let src = "name_of : { name: Str, .. } -> Str\nname_of = |r| r.name\n\
               [name_of({ name: \"a\", age: 1 }), name_of({ name: \"b\" })]";
    assert_eq!(value(src), "[\"a\", \"b\"]");
}

#[test]
fn a_where_clause_permits_dispatch_on_a_type_variable() {
    // Without the clause this is "Cannot dispatch `to_str` on an unresolved type".
    let src = "label : a -> Str where [a.to_str : a -> Str]\nlabel = |x| x.to_str()\n\
               n : I64\nn = 7\nlabel(n)";
    assert_eq!(value(src), "\"7\"");
}

#[test]
fn a_parameterised_nominal_instantiates_its_backing_type() {
    let src = "Wrapper(a) := { item: a }\nunwrap : Wrapper(a) -> a\nunwrap = |w| w.item\n\
               n : Wrapper(I64)\nn = Wrapper.{ item: 42 }\nunwrap(n) + 1";
    assert_eq!(value(src), "43");
}
