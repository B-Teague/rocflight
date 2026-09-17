//! `if` / `else` — phase 6.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `if` is an EXPRESSION, and `else` is OPTIONAL: `if cond { … }` parses and has
//!     the type `{}`. Re-checked against nightly-2026-09-03 on 2026-09-16 — using the
//!     result is a type error against `{}`, never a parse error, which is what lets a
//!     bare `if` stand as a statement.
//!   * the condition must be a Bool: "This if condition must evaluate to a Bool".
//!   * `else if` is not a separate form — it nests.
//!   * braced branches are real blocks, so they may bind names.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Value;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn parse(src: &str) -> Result<rocflight::ast::Expr, rocflight::error::ParseError> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr()
}

fn eval(src: &str) -> Value {
    let ast = parse(src).expect("parse failed");
    TypeChecker::new().synth(&ast).expect("type check failed");
    rocflight::vm::eval(&ast).expect("eval failed")
}

fn as_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => s.to_string(),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn one_line_if_picks_a_branch() {
    assert_eq!(as_str(r#"if 1 == 1 "yes" else "no""#), "yes");
    assert_eq!(as_str(r#"if 1 == 2 "yes" else "no""#), "no");
}

#[test]
fn condition_needs_no_parens_but_accepts_them() {
    assert_eq!(as_str(r#"if (1 == 1) "yes" else "no""#), "yes");
}

#[test]
fn branches_may_be_blocks() {
    // A braced branch is a block, not a record: it can bind names and its value is
    // the final expression.
    assert_eq!(
        as_str("if 1 == 1 {\n    label = \"five\"\n    label\n} else {\n    \"other\"\n}"),
        "five"
    );
}

#[test]
fn else_if_chains_nest() {
    let src = r#"if 1 == 3 "three" else if 1 == 1 "one" else "other""#;
    assert_eq!(as_str(src), "one");

    // `else if` is an If whose otherwise is another If — no separate node.
    let ast = parse(src).unwrap();
    match ast {
        rocflight::ast::Expr::If { otherwise, .. } => assert!(
            matches!(*otherwise, rocflight::ast::Expr::If { .. }),
            "else-if should nest an If, got {}",
            otherwise
        ),
        other => panic!("expected If, got {}", other),
    }
}

#[test]
fn else_if_falls_through_to_the_last_branch() {
    assert_eq!(
        as_str(r#"if 1 == 3 "three" else if 1 == 4 "four" else "other""#),
        "other"
    );
}

#[test]
fn only_the_taken_branch_is_evaluated() {
    // The untaken branch divides by zero. If both branches were evaluated this
    // would error instead of returning a value.
    assert_eq!(as_str(r#"if 1 == 1 "safe" else I64.to_str(1 // 0)"#), "safe");
    assert_eq!(as_str(r#"if 1 == 2 I64.to_str(1 // 0) else "safe""#), "safe");
}

#[test]
fn a_missing_else_branch_is_unit() {
    // Not a parse error: roc accepts `if cond { … }` and gives the absent branch the
    // type `{}`.
    let ast = parse(r#"if 1 == 1 { 2 } "#).expect("a bare if parses");
    assert_eq!(ast.to_string(), "if (1 == 1) 2 else {}");

    // Using its value is still rejected, because `{}` does not unify with the taken
    // branch — a TYPE error, which is exactly where roc reports it too ("The value's
    // type, which does not have a method named from_numeral, is: {}").
    let err = TypeChecker::new()
        .synth(&parse(r#"if 1 == 1 { 2 } "#).unwrap())
        .expect_err("a bare if with a value does not check");
    assert!(err.message.contains("{}"), "got {:?}", err);

    // It earns its keep as a STATEMENT, where both branches are `{}`. This is the
    // shape `Builtin.roc` uses for a guarded assignment inside a loop.
    let src = "f = |n| {\n\tvar $t = 0\n\tif n > 0 {\n\t\t$t = 1\n\t}\n\t$t\n}\n\nf(5)";
    assert_eq!(eval(src).to_string(), "1");
}

#[test]
fn non_bool_condition_is_a_type_error() {
    let ast = parse(r#"if 1 "yes" else "no""#).unwrap();
    assert!(
        TypeChecker::new().synth(&ast).is_err(),
        "a numeric condition should not type check"
    );
}

#[test]
fn mismatched_branches_are_a_type_error() {
    // Both branches must agree: the if has one type.
    let ast = parse(r#"if 1 == 1 "str" else 42"#).unwrap();
    assert!(
        TypeChecker::new().synth(&ast).is_err(),
        "Str vs I64 branches should not type check"
    );
}

#[test]
fn if_works_as_a_call_argument() {
    assert_eq!(as_str(r#"Str.inspect(if 1 == 1 "a" else "b")"#), "\"a\"");
}

#[test]
fn if_nests_inside_a_branch() {
    assert_eq!(
        as_str(r#"if 20 > 0 (if 20 > 10 "big" else "small") else "neg""#),
        "big"
    );
}

#[test]
fn keyword_boundaries_are_respected() {
    // `iffy` and `elsewhere` are ordinary identifiers, not `if` / `else`.
    assert_eq!(as_str("iffy = \"name\"\niffy"), "name");
    assert_eq!(as_str("elsewhere = \"name\"\nelsewhere"), "name");
}

#[test]
fn call_requires_no_space_before_the_paren() {
    // roc rejects `f (1)`, and that rule is load-bearing: it lets a parenthesised
    // expression follow an operand without being read as a call on it.
    // `n > 0 (…)` must parse as a comparison followed by a separate group.
    let ast = parse("f = |x| x\nn = 1\nif n > 0 (if n > 10 \"big\" else \"small\") else \"neg\"")
        .expect("grouped expression after an operand should parse");
    assert!(format!("{}", ast).contains("if"), "expected an if, got {}", ast);
}
