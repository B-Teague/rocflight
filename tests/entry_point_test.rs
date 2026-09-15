//! The platformless-app entry-point model.
//!
//! Facts locked in here were verified against `roc` nightly-2026-09-03 and
//! `roc experimental-lsp`, not assumed:
//!   * `main! = |_args| { ... }` with no header is a valid app; the header
//!     `app [main!] {}` is implied.
//!   * `echo!` comes from the default host, and is `Str => {}`.
//!   * The `!` is part of the identifier, so names are looked up verbatim.

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;

fn entry_point_of(src: &str) -> Option<String> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    parser.parse_expr().unwrap();
    parser.app_entry_point()
}

#[test]
fn headerless_app_implies_main_bang() {
    let entry = entry_point_of("main! = |_args| {\n    echo!(\"hi\")\n    Ok({})\n}\n");
    assert_eq!(entry.as_deref(), Some("main!"));
}

#[test]
fn explicit_empty_header_is_accepted() {
    // `app [main!] {}` has no platform string. The header parser must not go
    // looking for one.
    let entry = entry_point_of("app [main!] {}\n\nmain! = |_| {\n    echo!(\"hi\")\n    Ok({})\n}\n");
    assert_eq!(entry.as_deref(), Some("main!"));
}

#[test]
fn annotations_do_not_truncate_the_binding_chain() {
    // Regression: an unskipped `main! : ...` line ended the top-level chain, so
    // `main!` was never bound and lookup failed at run time.
    let src = "app [main!] {}\n\
               \n\
               greeting : Str\n\
               greeting = \"hello\"\n\
               \n\
               main! : List(Str) => Try({}, [Exit(I8), ..])\n\
               main! = |_args| {\n    echo!(greeting)\n    Ok({})\n}\n";
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();

    // The annotations survive desugaring...
    assert!(desugared.contains("greeting : Str"), "stripped annotation");
    assert!(desugared.contains("main! : List(Str)"), "stripped annotation");

    // ...and the parser still reaches the `main!` binding past them.
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().unwrap();
    assert!(
        format!("{}", ast).contains("main!"),
        "binding chain truncated at the annotation: {}",
        ast
    );
}

#[test]
fn record_fields_are_not_mistaken_for_annotations() {
    // `name: value` (no space) is a record field; `name : Type` is an annotation.
    // Conflating them would silently delete record fields.
    let src = "app [main!] {}\n\nmain! = |_| {\n    r = 1\n    r\n}\n";
    assert_eq!(entry_point_of(src).as_deref(), Some("main!"));
}

#[test]
fn echo_is_a_host_effect_not_a_builtin() {
    use rocflight::platform::host;

    // Confirmed by LSP hover: `echo! : Str => {}`.
    assert_eq!(host::lookup("echo!"), Some((&["Str"][..], "{}")));
    // Dropping the `!` gives a different, unknown name.
    assert!(host::lookup("echo").is_none());
}

// ---------------------------------------------------------------------------
// Expressions must be parsed at full precedence wherever they can appear.
// Three places used a lower rung of the ladder and silently truncated:
// argument lists, top-level binding values, and block statements.
// ---------------------------------------------------------------------------

fn eval_ok(src: &str) -> String {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    let mut parser = Parser::new(&desugared);
    let ast = parser.parse_expr().expect("parse failed");
    let mut tc = rocflight::types::TypeChecker::new();
    tc.synth(&ast).expect("type check failed");
    let mut ev = rocflight::eval::Evaluator::new();
    ev.eval(&ast).expect("eval failed").to_string()
}

#[test]
fn call_can_be_an_argument_to_a_call() {
    // Regression: arguments were parsed with `parse_primary_expr`, a single atom,
    // so `inc(41)` inside `I64.to_str(...)` stopped after `inc` and the stray `(`
    // failed the closing-paren check.
    assert_eq!(eval_ok("inc = |x| x + 1\nI64.to_str(inc(41))"), "\"42\"");
}

#[test]
fn operator_can_be_an_argument_to_a_call() {
    assert_eq!(eval_ok("f = |x| x * 2\nI64.to_str(f(20 + 1))"), "\"42\"");
}

#[test]
fn top_level_binding_value_can_be_an_operator_expression() {
    // Regression: top-level binding values used `parse_call_expr`, which has no
    // operator handling, so `a = 2 + (3 * 4)` failed — while the same binding
    // inside a block worked. Only the desugared test files exposed this, because
    // they lift bindings to the top level.
    assert_eq!(eval_ok("a = 2 + (3 * 4)\na"), "14");
}

#[test]
fn block_statements_may_be_annotated() {
    // Regression: `skip_trivia` was not called inside blocks, so an annotation on
    // a block-local binding was parsed as an expression.
    let src = "main! = |_| {\n    n : I64\n    n = 21\n    n * 2\n}\n";
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    assert!(desugared.contains("n : I64"), "annotation stripped");
    let mut parser = Parser::new(&desugared);
    parser.parse_expr().expect("annotation inside block broke parsing");
}

#[test]
fn calling_a_non_function_is_still_an_error() {
    // Loosening the Call arm to unify instead of pattern-matching must not make
    // every callee acceptable. Now that identifiers have types, a named non-function
    // is caught too — `x = 42` then `x(1)` used to slip through.
    for src in ["42(1)", "\"hi\"(1)", "x = 42\nx(1)", "s = \"hi\"\ns(1)"] {
        let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
        let mut parser = Parser::new(&desugared);
        let ast = parser.parse_expr().unwrap();
        let mut tc = rocflight::types::TypeChecker::new();
        assert!(tc.synth(&ast).is_err(), "{} should not type check", src);
    }
}

#[test]
fn identifiers_now_carry_their_type() {
    // This replaces a test that documented the opposite: identifiers used to synth to
    // a fresh type variable, so the checker knew nothing about them. With a type
    // environment, a name's type is available wherever it is used.
    use rocflight::types::TypeChecker;

    let typed = |src: &str| -> String {
        let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
        let ast = Parser::new(&desugared).parse_expr().unwrap();
        TypeChecker::new().synth(&ast).map(|t| t.to_string()).unwrap_or_default()
    };

    assert_eq!(typed("x = 42\nx"), "I64");
    assert_eq!(typed("s = \"hi\"\ns"), "Str");
    assert_eq!(typed("b = Bool.True\nb"), "Bool");
}
