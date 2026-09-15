//! Loops and mutable bindings — phase 16.
//!
//! Verified against `roc` nightly-2026-09-03 before implementing:
//!   * `$` is PART of the identifier — `$sum` and `sum` are different names, the same
//!     way a trailing `!` distinguishes an effectful name
//!   * a plain binding cannot be reassigned; roc reports it as a redeclaration
//!   * a loop's value is `{}` — it is a statement, not something you bind
//!   * `break` leaves the nearest enclosing loop
//!
//! NOT implemented: `continue`. It CRASHES the roc compiler on nightly-2026-09-03
//! ("Please report this issue at github.com/roc-lang/roc/issues"), so no golden pair
//! can be written against it.

use rocflight::desugaring::Desugarer;
use rocflight::eval::Evaluator;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

fn build(src: &str) -> rocflight::ast::Expr<'static> {
    let desugared = Desugarer::new(src.to_string()).desugar().unwrap();
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

fn value(src: &str) -> String {
    let ast = build(src);
    TypeChecker::new().synth(&ast).expect("type check failed");
    Evaluator::new().eval(&ast).expect("eval failed").to_string()
}

fn eval_error(src: &str) -> String {
    let ast = build(src);
    let _ = TypeChecker::new().synth(&ast);
    Evaluator::new().eval(&ast).expect_err("expected an eval error").message
}

// --- var and assignment ---------------------------------------------------

#[test]
fn a_var_can_be_reassigned() {
    assert_eq!(
        value("f = |_| {\n    var $a = 5\n    $a = $a * 2\n    $a\n}\nf(0)"),
        "10"
    );
}

#[test]
fn the_dollar_is_part_of_the_name() {
    // `$x` and `x` are different names, so this reads the outer `x`, not the var.
    assert_eq!(
        value("f = |_| {\n    x = 1\n    var $x = 99\n    x\n}\nf(0)"),
        "1"
    );
}

#[test]
fn a_dollar_name_without_var_is_an_ordinary_binding() {
    // `$` carries no meaning of its own — roc accepts `$nope = 1` as a plain binding
    // and prints 1. Mutability comes from `var`, not from the sigil.
    assert_eq!(value("f = |_| {\n    $nope = 1\n    $nope\n}\nf(0)"), "1");
}

#[test]
fn a_var_without_a_dollar_is_still_reassignable() {
    assert_eq!(
        value("f = |xs| {\n    var sum = 0\n    for n in xs {\n        sum = sum + n\n    }\n    sum\n}\nf([1, 2, 3])"),
        "6"
    );
}

// --- for ------------------------------------------------------------------

#[test]
fn a_for_loop_accumulates_into_a_var() {
    // The body runs in its own scope, so the assignment has to UPDATE the outer var
    // rather than bind a new one — otherwise the sum is lost when the scope pops.
    assert_eq!(
        value("f = |xs| {\n    var $sum = 0\n    for n in xs {\n        $sum = $sum + n\n    }\n    $sum\n}\nf([1, 2, 3, 4])"),
        "10"
    );
}

#[test]
fn an_empty_list_leaves_the_var_untouched() {
    assert_eq!(
        value("f = |xs| {\n    var $sum = 7\n    for n in xs {\n        $sum = $sum + n\n    }\n    $sum\n}\nf([])"),
        "7"
    );
}

#[test]
fn a_for_loop_evaluates_to_unit() {
    assert_eq!(
        value("f = |xs| {\n    var $s = 0\n    y = for n in xs {\n        $s = $s + n\n    }\n    y\n}\nf([1])"),
        "{}"
    );
}

#[test]
fn the_loop_variable_is_scoped_to_the_body() {
    let err = eval_error("f = |xs| {\n    for n in xs {\n        n\n    }\n    n\n}\nf([1])");
    assert!(err.contains("Undefined"), "got {}", err);
}

#[test]
fn iterating_a_non_list_is_an_error() {
    let err = eval_error("f = |_| {\n    for n in 5 {\n        n\n    }\n    1\n}\nf(0)");
    assert!(err.contains("List"), "got {}", err);
}

// --- while ----------------------------------------------------------------

#[test]
fn a_while_loop_runs_until_its_condition_is_false() {
    assert_eq!(
        value("f = |limit| {\n    var $i = 0\n    var $sum = 0\n    while $i < limit {\n        $sum = $sum + $i\n        $i = $i + 1\n    }\n    $sum\n}\nf(5)"),
        "10"
    );
}

#[test]
fn a_while_loop_may_never_run() {
    assert_eq!(
        value("f = |limit| {\n    var $i = 0\n    while $i < limit {\n        $i = $i + 1\n    }\n    $i\n}\nf(0)"),
        "0"
    );
}

#[test]
fn a_non_bool_while_condition_is_an_error() {
    let err = eval_error("f = |_| {\n    var $i = 0\n    while $i {\n        $i = 1\n    }\n    $i\n}\nf(0)");
    assert!(err.contains("Bool"), "got {}", err);
}

// --- break ----------------------------------------------------------------

#[test]
fn break_exits_a_for_loop_early() {
    assert_eq!(
        value("f = |xs| {\n    var $found = 0\n    for n in xs {\n        if n < 0 {\n            $found = n\n            break\n        } else {\n            {}\n        }\n    }\n    $found\n}\nf([1, 2, 0 - 7, 3])"),
        "-7"
    );
}

#[test]
fn break_exits_a_while_loop_early() {
    assert_eq!(
        value("f = |_| {\n    var $i = 0\n    while Bool.True {\n        $i = $i + 1\n        if $i > 3 {\n            break\n        } else {\n            {}\n        }\n    }\n    $i\n}\nf(0)"),
        "4"
    );
}

#[test]
fn break_outside_a_loop_surfaces_as_an_error() {
    // It rides the error channel, so it must not vanish silently when there is no
    // loop to catch it.
    let err = eval_error("f = |_| {\n    break\n}\nf(0)");
    assert!(!err.is_empty(), "a stray break should not be swallowed");
}

// --- the bug this exposed -------------------------------------------------

#[test]
fn an_assignment_as_the_last_statement_of_a_block_still_runs() {
    // The fold used to discard the final statement's binding target, so a loop body
    // whose only statement was an assignment did nothing at all.
    assert_eq!(
        value("f = |_| {\n    var $a = 1\n    for n in [10] {\n        $a = n\n    }\n    $a\n}\nf(0)"),
        "10"
    );
}
