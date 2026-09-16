//! Unary minus — phase 07.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `-x` IS `x.negate()` — roc lowers it to that method, which is why `-s` on a Str
//!     fails with "This negate method is being called on a value whose type doesn't
//!     have that method" rather than a syntax error
//!   * the operand is the whole POSTFIX chain: `-r.v` is `-(r.v)`, and `-n.to_str()`
//!     tries to negate a Str
//!   * it is LOOSER than `|>`: `-n |> inc` is `-(inc(n))` = -6, not `inc(-n)` = -4
//!
//! Known divergence: roc's tokenizer rejects a `-` bound tightly to an identifier
//! (`m-n` and `m -n` are parse errors there, while `m - n` and `10-3` are fine). This
//! interpreter accepts all of them as subtraction — more permissive, which is the safe
//! direction, and no golden pair can rely on it because every pair passes `roc check`.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Evaluator;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn value(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    Evaluator::new().eval(&ast).expect("eval failed").to_string()
}

const DEFS: &str = "n : I64\nn = 5\nm : I64\nm = 3\ninc : I64 -> I64\ninc = |x| x + 1\nscale : I64 -> I64\nscale = |x| x * 10\n";

fn expr(e: &str) -> String {
    value(&format!("{}{}", DEFS, e))
}

// --- the lowering ---------------------------------------------------------

#[test]
fn unary_minus_lowers_to_a_negate_call() {
    // `-x` and `x.negate()` must build the SAME AST — that is what the golden pair
    // demonstrates, and it is why the diagnostic for `-"s"` mentions a method.
    let sugared = format!("{}", build("n = 5\n-n"));
    let explicit = format!("{}", build("n = 5\nn.negate()"));
    assert_eq!(sugared, explicit);
    assert!(sugared.contains("negate"), "expected a negate call, got {}", sugared);
}

#[test]
fn negating_a_variable() {
    assert_eq!(expr("-n"), "-5");
}

#[test]
fn a_negative_literal_stays_one_token() {
    // `-5` is lexed as a literal, not a negate call on 5.
    assert_eq!(value("-5"), "-5");
    assert!(!format!("{}", build("-5")).contains("negate"));
}

// --- the operand is the whole postfix chain -------------------------------

#[test]
fn the_operand_includes_field_access() {
    assert_eq!(expr("r = { v: 4 }\n-r.v"), "-4");
}

#[test]
fn the_operand_includes_a_call() {
    assert_eq!(expr("-scale(2)"), "-20");
}

#[test]
fn the_operand_includes_a_parenthesised_expression() {
    assert_eq!(expr("-(1 + 2)"), "-3");
}

#[test]
fn negating_a_non_number_is_an_error() {
    // roc reports this as a missing `negate` method, not a syntax error.
    let ast = build("s : Str\ns = \"x\"\n-s");
    let _ = TypeChecker::new().synth(&ast);
    assert!(Evaluator::new().eval(&ast).is_err(), "negating a Str should fail");
}

// --- precedence -----------------------------------------------------------

#[test]
fn unary_minus_is_tighter_than_the_binary_operators() {
    assert_eq!(expr("2 * -n"), "-10");
    assert_eq!(expr("10 - -n"), "15");
    assert_eq!(expr("-n + 1"), "-4");
}

#[test]
fn unary_minus_is_looser_than_a_pipeline() {
    // `-(inc(n))` = -6, not `inc(-n)` = -4. The two readings differ, so this is a real
    // test rather than a coincidence of the numbers.
    assert_eq!(expr("-n |> inc"), "-6");
}

#[test]
fn binary_minus_still_works() {
    assert_eq!(expr("m - n"), "-2");
    assert_eq!(expr("m - -n"), "8");
}

#[test]
fn negation_nests() {
    assert_eq!(expr("-(-n)"), "5");
}

#[test]
fn negation_composes_with_dispatch() {
    // `(-n).to_str()` negates first; `-n.to_str()` would negate a Str, which fails.
    assert_eq!(expr("(-n).to_str()"), "\"-5\"");
}
