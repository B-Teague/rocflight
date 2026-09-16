//! The register VM, phase V0 — a DIFFERENTIAL test against the tree-walker.
//!
//! This is the gate for the VM phases. Nothing here asserts a value it decided was
//! right: each program is run by both engines and the two answers must agree. That is
//! the property that matters while the VM is being built alongside a tree-walker that
//! already passes 98 golden pairs against `roc` — if the VM matches the tree-walker and
//! the tree-walker matches `roc`, the VM matches `roc`.
//!
//! The subset so far is literals, locals, arithmetic, `if`, `let`, `return`, closures
//! with captures, functions as values, and calls — direct, dynamic and tail. Anything
//! else must be a compile ERROR rather than a wrong answer, which
//! `unsupported_is_an_error_not_a_wrong_answer` checks.

use rocflight::ast::Expr;
use rocflight::desugaring::Desugarer;
use rocflight::eval::Evaluator;
use rocflight::parser::Parser;
use rocflight::vm;

fn parse(src: &str) -> Expr {
    let desugared = Desugarer::new(src.to_string()).desugar().expect("desugar failed");
    Parser::new(&desugared).parse_expr().expect("parse failed")
}

/// Both engines run `src`; their answers must be identical.
fn agree(src: &str) -> String {
    let ast = parse(src);
    let walked = Evaluator::new().eval(&ast).expect("tree-walker failed").to_string();
    let program = std::rc::Rc::new(vm::compile(&ast, None).unwrap_or_else(|e| panic!("vm compile failed: {}", e)));
    let ran = vm::run(&program).expect("vm failed").to_string();
    assert_eq!(walked, ran, "the two engines disagree on:\n{}", src);
    ran
}

#[test]
fn literals() {
    assert_eq!(agree("42"), "42");
    assert_eq!(agree("-7"), "-7");
    assert_eq!(agree("3.5"), "3.5");
    // `Display` for a Str quotes it, which is what `roc` prints for a bare value.
    assert_eq!(agree("\"hello\""), "\"hello\"");
    assert_eq!(agree("{}"), "{}");
}

#[test]
fn arithmetic() {
    assert_eq!(agree("2 + 3 * 4"), "14");
    assert_eq!(agree("(2 + 3) * 4"), "20");
    assert_eq!(agree("7 - 9"), "-2");
    assert_eq!(agree("7.0 / 2.0"), "3.5");
    assert_eq!(agree("7 // 2"), "3");
    assert_eq!(agree("-7 // 2"), "-3");
    assert_eq!(agree("7 % 3"), "1");
    assert_eq!(agree("2 + 3.5"), "5.5");
}

#[test]
fn comparison_and_logic() {
    assert_eq!(agree("1 < 2"), "True");
    assert_eq!(agree("2 <= 2"), "True");
    assert_eq!(agree("3 > 4"), "False");
    assert_eq!(agree("3 >= 4"), "False");
    assert_eq!(agree("1 == 1"), "True");
    assert_eq!(agree("1 != 1"), "False");
    assert_eq!(agree("1 < 2 && 2 < 3"), "True");
    assert_eq!(agree("1 < 2 || 3 < 2"), "True");
}

#[test]
fn strings_concatenate() {
    // No string BUILTINS in V0, but `+` on two strings goes through the same operator
    // table as everything else, so it comes along for free.
    assert_eq!(agree("\"ab\" + \"cd\""), "\"abcd\"");
}

#[test]
fn locals_and_shadowing_in_blocks() {
    assert_eq!(agree("x = 5\n\nx + 1"), "6");
    assert_eq!(agree("x = 5\ny = x * 2\n\nx + y"), "15");
    // An inner block's binding is its own register and must not leak out.
    assert_eq!(
        agree("outer = 1\n\nf = |n| {\n\tinner = n * 10\n\tinner + outer\n}\n\nf(2)"),
        "21"
    );
}

#[test]
fn conditionals() {
    assert_eq!(agree("if 1 < 2 { 10 } else { 20 }"), "10");
    assert_eq!(agree("if 1 > 2 { 10 } else { 20 }"), "20");
    // Nested, and with a binding inside a branch: both branches must land their result
    // in the same register whichever way the jump went.
    assert_eq!(
        agree("n = 5\n\nif n < 0 { 0 } else { if n < 10 { d = n * 2\n\td + 1 } else { 99 } }"),
        "11"
    );
}

#[test]
fn only_the_taken_branch_runs() {
    // A division by zero in the untaken branch is an error if it is evaluated. The
    // tree-walker takes one branch; so must the VM.
    assert_eq!(agree("if 1 < 2 { 1 } else { 1 // 0 }"), "1");
}

#[test]
fn calls() {
    assert_eq!(agree("double = |x| x * 2\n\ndouble(21)"), "42");
    assert_eq!(agree("add = |a, b| a + b\n\nadd(1, 2)"), "3");
    // Argument order, with each argument itself a call: the arguments have to land in
    // consecutive registers without clobbering each other.
    assert_eq!(
        agree("sub = |a, b| a - b\n\nsub(sub(10, 3), sub(4, 1))"),
        "4"
    );
    // A function declared BELOW its caller, which is what the id-before-compile pass
    // in the compiler is for.
    assert_eq!(agree("caller = |x| helper(x) + 1\nhelper = |x| x * 3\n\ncaller(5)"), "16");
}

#[test]
fn recursion() {
    assert_eq!(
        agree("fib = |n| if n < 2 { n } else { fib(n - 1) + fib(n - 2) }\n\nfib(20)"),
        "6765"
    );
    // Mutual recursion, which needs both chunk ids to exist before either body is
    // compiled.
    assert_eq!(
        agree(
            "is_even = |n| if n == 0 { 1 } else { is_odd(n - 1) }\n\
             is_odd = |n| if n == 0 { 0 } else { is_even(n - 1) }\n\n\
             is_even(10)"
        ),
        "1"
    );
}

#[test]
fn a_global_can_be_a_computed_value() {
    assert_eq!(agree("base = 6 * 7\n\nbump = |n| n + base\n\nbump(1)"), "43");
}

#[test]
fn deep_recursion_does_not_touch_the_rust_stack() {
    // The tree-walker needs a 256 MB thread stack to recurse a few hundred levels,
    // because one Roc call costs many Rust frames. The VM's frames are a `Vec`, so
    // this runs on an ordinary test thread's stack — which is the point, and which is
    // why this test does NOT run the tree-walker on it.
    let ast = parse("countdown = |n| if n == 0 { 0 } else { countdown(n - 1) }\n\ncountdown(100000)");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "0");
}

#[test]
fn what_the_vm_refuses_is_refused_deliberately() {
    // Through V0-V4 this test listed constructs the VM could not compile yet. It can
    // now compile all of them, so what is left is the short list of things it refuses
    // ON PURPOSE — each because compiling it would mean disagreeing with the
    // tree-walker rather than failing loudly. Each has its own test below for the
    // reason; this one is the inventory.
    for (src, expected) in [
        // A `var` a closure captures needs a shared cell.
        (
            "f = || {\n\tvar n = 0\n\tbump = |x| x + n\n\tn = 10\n\tbump(1)\n}\n\nf()",
            "shared cell",
        ),
        // The same hazard, the other way round.
        (
            "f = || {\n\tvar n = 0\n\tbump = |x| x + n\n\tn = 1\n\tbump(1)\n}\n\nf()",
            "shared cell",
        ),
        // `break` belongs to a loop in the same function.
        ("f = || break\n\nf()", "break"),
        // `return` belongs to a function.
        ("return 1", "return"),
    ] {
        let ast = parse(src);
        let err = vm::compile(&ast, None)
            .expect_err(&format!("the VM compiled `{}`, which it should refuse", src));
        assert!(err.contains(expected), "on `{}`: unexpected message {}", src, err);
    }
}

#[test]
fn an_if_condition_must_be_a_bool() {
    // The same refusal as the tree-walker's, including the message: `roc` has no
    // truthiness, and a VM that invented some would diverge silently.
    let ast = parse("if 1 { 2 } else { 3 }");
    let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    let ran = vm::run(&program).expect_err("vm accepted it");
    assert_eq!(walked.to_string(), ran.to_string());
}

// ---------------------------------------------------------------- V1: closures

#[test]
fn a_closure_captures_by_value() {
    assert_eq!(agree("make = |n| |x| x + n\n\nadd5 = make(5)\n\nadd5(3)"), "8");
    // Two closures from the same function must not share a capture.
    assert_eq!(
        agree("make = |n| |x| x + n\n\na = make(10)\nb = make(100)\n\na(1) + b(1)"),
        "112"
    );
}

#[test]
fn a_capture_threads_through_intervening_functions() {
    // `inner` uses `a`, which belongs to `outer` — two levels up. Every function in
    // between has to capture it as well, or the innermost cannot reach it.
    assert_eq!(
        agree(
            "outer = |a| {\n\
             \tmiddle = |b| {\n\
             \t\tinner = |c| a + b + c\n\
             \t\tinner(3)\n\
             \t}\n\
             \tmiddle(2)\n\
             }\n\n\
             outer(1)"
        ),
        "6"
    );
}

#[test]
fn functions_are_values() {
    assert_eq!(agree("double = |x| x * 2\napply = |f, x| f(x)\n\napply(double, 21)"), "42");
    // A closure passed to a function that calls it: the callee is a register, not a
    // known chunk, so this is the dynamic call path.
    assert_eq!(
        agree("twice = |f, x| f(f(x))\n\nbump = |n| |x| x + n\n\ntwice(bump(3), 1)"),
        "7"
    );
    // A function value renders the same on both engines, which is what keeps a
    // program's OUTPUT from revealing which engine ran it.
    assert_eq!(agree("f = |x, y| x + y\n\nf"), "<lambda |x, y|>");
    assert_eq!(agree("|x| x"), "<lambda |x|>");
}

#[test]
fn a_block_local_function_can_call_itself() {
    // `go` is not bound yet when its own closure is made, so it cannot be captured.
    // The running closure is in the frame instead.
    assert_eq!(
        agree(
            "sum_to = |n| {\n\
             \tgo = |k, acc| if k == 0 { acc } else { go(k - 1, acc + k) }\n\
             \tgo(n, 0)\n\
             }\n\n\
             sum_to(100)"
        ),
        "5050"
    );
}

#[test]
fn return_leaves_the_function() {
    assert_eq!(agree("f = |n| { return n * 2 }\n\nf(21)"), "42");
    assert_eq!(
        agree("f = |n| { if n < 0 { return 0 } else { n } }\n\nf(-5) + f(5)"),
        "5"
    );
}

#[test]
fn a_tail_call_reuses_its_frame() {
    // Five million tail calls. Without frame reuse this is five million frames and
    // fails on `MAX_FRAMES`; the tree-walker cannot run it at all, which is why this
    // case is VM-only.
    let ast = parse("count = |n, acc| if n == 0 { acc } else { count(n - 1, acc + 1) }\n\ncount(5000000, 0)");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "5000000");
}

#[test]
fn calling_a_non_function_is_refused_by_both() {
    // A DELIBERATE divergence, and the only one in this file. Calling a bound name
    // that does not hold a function, `g(2)` where `g` is `1`, makes the tree-walker
    // say "Function 'g' not defined" (src/eval/mod.rs:575) — which is wrong twice
    // over: `g` is defined, and what is wrong with it is its value. The VM says
    // "Attempted to call a non-function value: 1", which is the message the
    // tree-walker itself uses when the callee is not a bare name.
    //
    // Both engines must REFUSE the program; copying a misleading message into the
    // engine that replaces the old one at V6 would be the wrong kind of parity.
    let src = "x = 1\n\nf = |g| g(2)\n\nf(x)";
    let ast = parse(src);
    assert!(Evaluator::new().eval(&ast).is_err(), "tree-walker accepted it");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    let ran = vm::run(&program).expect_err("vm accepted it");
    assert_eq!(ran.to_string(), "Runtime error: Attempted to call a non-function value: 1");
}

#[test]
fn a_wrong_arity_call_says_so_the_same_way() {
    // Through a value, so both engines check it at RUN time and must word it alike.
    // A direct call is checked by the compiler instead, and says which function.
    let src = "f = |a, b| a + b\n\ncall1 = |g| g(1)\n\ncall1(f)";
    let ast = parse(src);
    let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    let ran = vm::run(&program).expect_err("vm accepted it");
    assert_eq!(walked.to_string(), ran.to_string());
}

#[test]
fn return_outside_a_function_is_refused() {
    // The tree-walker unwinds a `return` to the nearest call boundary, and at the top
    // level there is none. Refusing is never a wrong answer; guessing would be.
    let ast = parse("return 1");
    assert!(vm::compile(&ast, None).is_err());
}

// ------------------------------------------------- V2: aggregates and matching

#[test]
fn records() {
    assert_eq!(agree("p = { x: 1, y: 2 }\n\np.x + p.y"), "3");
    // Field order is source order, and rendering must agree on it.
    assert_eq!(agree("{ b: 2, a: 1 }"), "{ b: 2, a: 1 }");
    assert_eq!(agree("p = { x: 1, y: 2 }\n\nq = { ..p, x: 10 }\n\nq.x + q.y"), "12");
    assert_eq!(agree("{ inner: { deep: 7 } }.inner.deep"), "7");
    // An update returns a NEW record; roc has no mutation.
    assert_eq!(agree("p = { x: 1 }\n\nq = { ..p, x: 9 }\n\np.x + q.x"), "10");
}

#[test]
fn lists_and_tuples() {
    assert_eq!(agree("[1, 2, 3]"), "[1, 2, 3]");
    assert_eq!(agree("[]"), "[]");
    assert_eq!(agree("[[1], [2, 3]]"), "[[1], [2, 3]]");
    assert_eq!(agree("[1, 2] == [1, 2]"), "True");
    assert_eq!(agree("(\"Roc\", 1)"), "(\"Roc\", 1)");
    assert_eq!(agree("pair = (\"Roc\", 1)\n\npair.1"), "1");
    assert_eq!(agree("(1, (2, 3)).1.0"), "2");
}

#[test]
fn tags() {
    assert_eq!(agree("Red"), "Red");
    assert_eq!(agree("Ok(1)"), "Ok(1)");
    assert_eq!(agree("Outer(Inner(\"y\"))"), "Outer(Inner(\"y\"))");
    assert_eq!(agree("Wrap({ a: Red, b: True })"), "Wrap({ a: Red, b: True })");
    assert_eq!(agree("Ok(1) == Ok(1)"), "True");
}

#[test]
fn matching_literals() {
    assert_eq!(agree("match 2 {\n\t1 => \"one\"\n\t2 => \"two\"\n\t_ => \"other\"\n}"), "\"two\"");
    assert_eq!(agree("match \"b\" {\n\t\"a\" => 1\n\t\"b\" => 2\n\t_ => 3\n}"), "2");
    assert_eq!(agree("match 3.5 {\n\t3.5 => \"yes\"\n\t_ => \"no\"\n}"), "\"yes\"");
    // A literal pattern matches its own KIND only, where `1 == 1.0` is True. Both
    // engines go through `literal_pattern_matches` so they cannot drift on this.
    assert_eq!(agree("match 1.0 {\n\t1 => \"int\"\n\t_ => \"other\"\n}"), "\"other\"");
}

#[test]
fn matching_tags() {
    let src = "Shape : [Circle(I64), Rect(I64, I64), Dot]\n\n\
               area = |s| match s {\n\
               \tCircle(r) => 3 * r * r\n\
               \tRect(w, h) => w * h\n\
               \tDot => 0\n\
               }\n\n\
               area(Circle(2)) + area(Rect(6, 7)) + area(Dot)";
    assert_eq!(agree(src), "54");
    // Nested, and a payload the arm ignores.
    assert_eq!(
        agree("match Outer(Inner(5)) {\n\tOuter(Inner(n)) => n\n\t_ => 0\n}"),
        "5"
    );
    assert_eq!(agree("match Pair(1, 2) {\n\tPair(_, b) => b\n\t_ => 0\n}"), "2");
    // Same name, wrong arity: not a match.
    assert_eq!(agree("match Pair(1, 2) {\n\tPair(a) => a\n\t_ => 99\n}"), "99");
}

#[test]
fn matching_records_tuples_and_lists() {
    assert_eq!(agree("match { x: 1, y: 2 } {\n\t{ x, y } => x + y\n}"), "3");
    // A field the pattern names but the value lacks is not a match.
    assert_eq!(
        agree("match { x: 1 } {\n\t{ x: 1, y: 2 } => \"both\"\n\t{ x } => \"just x\"\n\t_ => \"no\"\n}"),
        "\"just x\""
    );
    // `..rest` binds the fields the pattern did not name, in the value's own order.
    assert_eq!(
        agree("match { a: 1, b: 2, c: 3 } {\n\t{ a, ..rest } => rest\n}"),
        "{ b: 2, c: 3 }"
    );
    assert_eq!(agree("match (0, 5) {\n\t(0, n) => n\n\t_ => 0\n}"), "5");
    assert_eq!(agree("match (1, 2, 3) {\n\t(a, b) => a\n\t(a, b, c) => c\n\t_ => 0\n}"), "3");

    let list_match = |list: &str| {
        format!(
            "describe = |xs| match xs {{\n\
             \t[] => \"empty\"\n\
             \t[a] => \"one\"\n\
             \t[1, 2, ..] => \"starts 1 2\"\n\
             \t[.. as most, last] => \"ends\"\n\
             }}\n\n\
             describe({})",
            list
        )
    };
    assert_eq!(agree(&list_match("[]")), "\"empty\"");
    assert_eq!(agree(&list_match("[9]")), "\"one\"");
    assert_eq!(agree(&list_match("[1, 2, 3]")), "\"starts 1 2\"");
    assert_eq!(agree(&list_match("[7, 8, 9]")), "\"ends\"");
    // `.. as name` binds the skipped middle.
    assert_eq!(
        agree("match [1, 2, 3, 4] {\n\t[first, .. as middle, last] => middle\n\t_ => []\n}"),
        "[2, 3]"
    );
}

#[test]
fn match_alternatives_and_guards() {
    assert_eq!(
        agree("match Green {\n\tRed | Green => \"warm-ish\"\n\t_ => \"other\"\n}"),
        "\"warm-ish\""
    );
    // A guard sees the pattern's bindings, and a false guard tries the NEXT arm
    // rather than failing the match.
    let guarded = |n: i64| {
        format!(
            "classify = |v| match v {{\n\
             \tn if n < 0 => \"neg\"\n\
             \t0 => \"zero\"\n\
             \tn if n > 100 => \"big\"\n\
             \t_ => \"pos\"\n\
             }}\n\n\
             classify({})",
            n
        )
    };
    assert_eq!(agree(&guarded(-1)), "\"neg\"");
    assert_eq!(agree(&guarded(0)), "\"zero\"");
    assert_eq!(agree(&guarded(500)), "\"big\"");
    assert_eq!(agree(&guarded(5)), "\"pos\"");
}

#[test]
fn an_unmatched_value_and_a_bad_guard_read_the_same() {
    // Both are errors roc's own exhaustiveness check would normally prevent, so the
    // wording is all the user gets.
    for src in [
        "match 5 {\n\t1 => \"one\"\n\t2 => \"two\"\n}",
        "match 5 {\n\tn if n => \"truthy?\"\n\t_ => \"no\"\n}",
    ] {
        let ast = parse(src);
        let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
        let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
        let ran = vm::run(&program).expect_err("vm accepted it");
        assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
    }
}

#[test]
fn aggregate_access_errors_read_the_same() {
    for src in [
        "p = { x: 1 }\n\np.nope",
        "p = 1\n\nf = |r| r.x\n\nf(p)",
        "p = { x: 1 }\n\nq = { ..p, y: 2 }\n\nq.y",
        "pair = (1, 2)\n\npair.5",
    ] {
        let ast = parse(src);
        let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
        let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
        let ran = vm::run(&program).expect_err("vm accepted it");
        assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
    }
}

#[test]
fn an_optional_field_is_a_try() {
    assert_eq!(agree("p = { x: 1 }\n\np.?x"), "Ok(1)");
    assert_eq!(agree("p = { x: 1 }\n\np.?y"), "Err(MissingField)");
}

#[test]
fn a_match_in_tail_position_returns_straight_out_of_its_arm() {
    // The whole function body is one match, so each arm ends the function: no result
    // register, no jump to a common exit. Ten million arms taken, in constant memory.
    let ast = parse(
        "step = |n| match n {\n\t0 => 0\n\tn => step(n - 1)\n}\n\nstep(1000000)",
    );
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "0");
}

// ------------------------------------------------------- V3: var and loops

#[test]
fn a_var_can_be_reassigned() {
    assert_eq!(agree("f = || {\n\tvar n = 1\n\tn = n + 1\n\tn = n * 10\n\tn\n}\n\nf()"), "20");
}

#[test]
fn for_over_a_range_and_a_list() {
    let sum = "f = |xs| {\n\tvar total = 0\n\tfor x in xs {\n\t\ttotal = total + x\n\t}\n\ttotal\n}\n\n";
    assert_eq!(agree(&format!("{}f([1, 2, 3])", sum)), "6");
    assert_eq!(agree(&format!("{}f([])", sum)), "0");
    assert_eq!(agree(&format!("{}f(0..<5)", sum)), "10");
    // `..=` includes its end, `..<` does not.
    assert_eq!(agree(&format!("{}f(1..=5)", sum)), "15");
    // An empty range runs the body zero times rather than once.
    assert_eq!(agree(&format!("{}f(3..<3)", sum)), "0");
}

#[test]
fn while_loops() {
    assert_eq!(
        agree("f = |n| {\n\tvar i = n\n\tvar steps = 0\n\twhile i > 0 {\n\t\ti = i - 1\n\t\tsteps = steps + 1\n\t}\n\tsteps\n}\n\nf(7)"),
        "7"
    );
    // A condition that is false to begin with runs the body zero times. Spelled as a
    // comparison: a bare `True`/`False` is a TAG here, which neither engine accepts as
    // a `while` condition.
    assert_eq!(
        agree("f = || {\n\tvar n = 0\n\twhile 1 > 2 {\n\t\tn = 1\n\t}\n\tn\n}\n\nf()"),
        "0"
    );
}

#[test]
fn break_leaves_the_innermost_loop() {
    assert_eq!(
        agree("f = |xs| {\n\tvar found = 0\n\tfor x in xs {\n\t\tif x > 2 { found = x\n\t\t\tbreak } else { 0 }\n\t}\n\tfound\n}\n\nf([1, 2, 5, 9])"),
        "5"
    );
    assert_eq!(
        agree("f = || {\n\tvar n = 0\n\tvar i = 0\n\twhile 1 < 2 {\n\t\ti = i + 1\n\t\tif i > 3 { break } else { n = n + i }\n\t}\n\tn\n}\n\nf()"),
        "6"
    );
    // An inner `break` leaves the INNER loop; the outer one keeps going.
    assert_eq!(
        agree("f = || {\n\tvar hits = 0\n\tfor _a in 0..<3 {\n\t\tfor _b in 0..<10 {\n\t\t\thits = hits + 1\n\t\t\tbreak\n\t\t}\n\t}\n\thits\n}\n\nf()"),
        "3"
    );
}

#[test]
fn a_range_is_not_a_list() {
    // roc keeps a range opaque, and both engines have to render it that way or a
    // program could tell them apart.
    assert_eq!(agree("0..<3"), "<opaque>");
    assert_eq!(agree("1..=3"), "<opaque>");
}

#[test]
fn a_range_is_iterated_without_being_built() {
    // Ten million elements would be 320 MB as a list of `Value`s. `IterNext` computes
    // each one instead, so this is constant memory — the same ceiling the tree-walker
    // removed by special-casing ranges in `for`.
    let ast = parse("f = || {\n\tvar total = 0\n\tfor i in 0..<10000000 {\n\t\ttotal = total + 1\n\t}\n\ttotal\n}\n\nf()");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "10000000");
}

#[test]
fn loop_errors_read_the_same() {
    for src in [
        // Not iterable.
        "f = || {\n\tvar n = 0\n\tfor x in 5 {\n\t\tn = x\n\t}\n\tn\n}\n\nf()",
        // A `while` condition must be a Bool.
        "f = || {\n\tvar n = 0\n\twhile 1 {\n\t\tn = 1\n\t}\n\tn\n}\n\nf()",
        // A range needs whole numbers.
        "f = || {\n\tvar n = 0\n\tfor x in 1.5..<3 {\n\t\tn = x\n\t}\n\tn\n}\n\nf()",
    ] {
        let ast = parse(src);
        let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
        let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
        let ran = vm::run(&program).expect_err("vm accepted it");
        assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
    }
}

#[test]
fn a_var_a_closure_captures_is_refused_rather_than_going_stale() {
    // Captured by value it would be a snapshot, and the tree-walker's shared frames
    // make it live — a silent disagreement. It needs a shared cell; nothing in roc's
    // own suite does this, so V3 refuses it instead of building one. If this test ever
    // has to change, the cell is the change.
    let ast = parse(
        "f = || {\n\tvar n = 0\n\tbump = |x| x + n\n\tn = 10\n\tbump(1)\n}\n\nf()",
    );
    let err = vm::compile(&ast, None).expect_err("the VM compiled a captured var");
    assert!(err.contains("shared cell"), "unexpected message: {}", err);
}

#[test]
fn assigning_an_undeclared_name_reads_the_same() {
    let src = "f = || {\n\tnope = nope + 1\n\tnope\n}\n\nf()";
    let ast = parse(src);
    assert!(Evaluator::new().eval(&ast).is_err(), "tree-walker accepted it");
    assert!(vm::compile(&ast, None).is_err(), "vm accepted it");
}

// ------------------------------- V4: strings, builtins and method dispatch

#[test]
fn string_interpolation() {
    assert_eq!(agree("x = 3\n\n\"n=${x}\""), "\"n=3\"");
    // A Str interpolates WITHOUT its quotes, unlike `Display`.
    assert_eq!(agree("s = \"hi\"\n\n\"[${s}]\""), "\"[hi]\"");
    assert_eq!(agree("a = 1\nb = 2\n\n\"${a},${b}\""), "\"1,2\"");
    // Nothing either side of the value, and a nested interpolation.
    assert_eq!(agree("x = 7\n\n\"${x}\""), "\"7\"");
    assert_eq!(agree("x = 7\n\n\"${\"${x}!\"}\""), "\"7!\"");
    // Aggregates render as `Display` does.
    assert_eq!(agree("p = { a: 1 }\n\n\"${p}\""), "\"{ a: 1 }\"");
}

#[test]
fn qualified_builtins() {
    assert_eq!(agree("Str.concat(\"ab\", \"cd\")"), "\"abcd\"");
    assert_eq!(agree("I64.to_str(42)"), "\"42\"");
    assert_eq!(agree("List.len([1, 2, 3])"), "3");
    assert_eq!(agree("Str.join_with([\"a\", \"b\"], \"-\")"), "\"a-b\"");
    // Bare `to_str` goes to the same place as `Num.to_str`.
    assert_eq!(agree("to_str(7)"), "\"7\"");
    // `Bool.True` is a VALUE, not a nullary builtin.
    assert_eq!(agree("Bool.True"), "True");
    assert_eq!(agree("{ x: Bool.True, y: Bool.False }"), "{ x: True, y: False }");
}

#[test]
fn method_dispatch_on_builtins() {
    assert_eq!(agree("x = 42\n\nx.to_str()"), "\"42\"");
    assert_eq!(agree("s = \"  hi  \"\n\ns.trim()"), "\"hi\"");
    assert_eq!(agree("[1, 2, 3].len()"), "3");
    assert_eq!(agree("\"a,b\".split_on(\",\")"), "[\"a\", \"b\"]");
}

#[test]
fn a_builtin_callback_can_be_a_vm_closure() {
    // The crux of V4: `List.map` is the TREE-WALKER's implementation, and the function
    // it calls back into is a VM closure. `eval::apply` re-enters the VM for it, which
    // is why forty builtins did not have to be written twice.
    assert_eq!(agree("[1, 2, 3].map(|x| x * 2)"), "[2, 4, 6]");
    assert_eq!(agree("[1, 2, 3, 4].fold(0, |a, x| a + x)"), "10");
    // A capture inside the callback, so the closure is a real one and not just a
    // top-level function in disguise.
    assert_eq!(agree("f = |n| [1, 2, 3].map(|x| x * n)\n\nf(10)"), "[10, 20, 30]");
    // A callback that itself calls a builtin that takes a callback.
    assert_eq!(
        agree("[[1, 2], [3]].map(|xs| xs.fold(0, |a, x| a + x))"),
        "[3, 3]"
    );
    // A top-level function passed by name, declared BELOW its use.
    assert_eq!(agree("go = || [1, 2].map(double)\ndouble = |x| x * 2\n\ngo()"), "[2, 4]");
    // A builtin passed as a value.
    assert_eq!(agree("[1, 2].map(Str.inspect)"), "[\"1\", \"2\"]");
}

#[test]
fn try_methods() {
    assert_eq!(agree("Ok(1).with_default(0)"), "1");
    assert_eq!(agree("Err(Nope).with_default(0)"), "0");
    assert_eq!(agree("Ok(2).map_ok(|x| x * 10)"), "Ok(20)");
    assert_eq!(agree("Err(Nope).map_ok(|x| x * 10)"), "Err(Nope)");
    assert_eq!(agree("Ok(1).is_ok()"), "True");
}

#[test]
fn a_nominal_method_is_resolved_at_compile_time() {
    let src = "Counter :: { n: I64 }.{\n\
               \tstart : Counter\n\
               \tstart = { n: 0 }\n\n\
               \tbump : Counter, I64 -> Counter\n\
               \tbump = |c, by| { ..c, n: c.n + by }\n\n\
               \tshow : Counter -> Str\n\
               \tshow = |c| c.n.to_str()\n\
               }\n\n\
               c = Counter.bump(Counter.start, 5)\n\n\
               \"${c.show()},${Counter.show(c)},${c.bump(2).show()}\"";
    assert_eq!(agree(src), "\"5,5,7\"");
}

#[test]
fn an_operator_dispatches_to_a_nominals_method() {
    // `a + b` IS `a.plus(b)`. The VM only emits the dispatching opcode for a program
    // that defines such a method, so ordinary arithmetic never pays for the lookup.
    let src = "Money :: { cents: I64 }.{\n\
               \tplus = |a, b| { cents: a.cents + b.cents }\n\
               \tminus = |a, b| { cents: a.cents - b.cents }\n\
               \tnegate = |a| { cents: 0 - a.cents }\n\
               \tis_eq = |a, b| a.cents == b.cents\n\
               }\n\n\
               a = Money.{ cents: 5 }\n\
               b = Money.{ cents: 7 }\n\n\
               \"${(a + b).cents},${(a - b).cents},${(-a).cents},${Str.inspect(a == b)}\"";
    assert_eq!(agree(src), "\"12,-2,-5,False\"");
}

#[test]
fn a_builtin_error_reads_the_same() {
    for src in [
        "x = 1\n\nx.nonexistent_method()",
        "List.len(1)",
        "[1].map(2)",
        "Str.trim(1)",
    ] {
        let ast = parse(src);
        let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
        let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
        let ran = vm::run(&program).expect_err("vm accepted it");
        assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
    }
}

// ------------------------------------ V5: expect, dbg, crash and multi-file

#[test]
fn expect_passes_and_carries_on() {
    // roc does not abort on a failed expectation: it reports and continues, so the
    // program's VALUE is unaffected either way. Both engines have to agree on that as
    // well as on the tally, which is process-wide and shared.
    assert_eq!(agree("expect 1 == 1\n\n42"), "42");
    assert_eq!(agree("f = |n| {\n\texpect n > 0\n\tn * 2\n}\n\nf(21)"), "42");
}

#[test]
fn expect_needs_a_bool() {
    let ast = parse("expect 1\n\n1");
    let walked = Evaluator::new().eval(&ast).expect_err("tree-walker accepted it");
    let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
    let ran = vm::run(&program).expect_err("vm accepted it");
    assert_eq!(walked.to_string(), ran.to_string());
}

#[test]
fn dbg_does_not_change_a_value() {
    // It prints to stderr and yields `{}`, so it is invisible to the program.
    assert_eq!(agree("f = |n| {\n\tdbg n\n\tn + 1\n}\n\nf(1)"), "2");
}

#[test]
fn crash_reads_the_same() {
    for src in [
        "crash \"nope\"",
        "f = |n| if n < 0 { crash \"negative\" } else { n }\n\nf(-1)",
        // A crash in the branch NOT taken must not happen at all.
        "f = |n| if n > 0 { n } else { crash \"unreachable\" }\n\nf(1)",
    ] {
        let ast = parse(src);
        let program = std::rc::Rc::new(vm::compile(&ast, None).expect("compile failed"));
        match Evaluator::new().eval(&ast) {
            Err(walked) => {
                let ran = vm::run(&program).expect_err("vm accepted it");
                assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
            }
            Ok(walked) => {
                let ran = vm::run(&program).expect("vm failed");
                assert_eq!(walked.to_string(), ran.to_string(), "on: {}", src);
            }
        }
    }
}

#[test]
fn a_module_is_compiled_into_the_same_program() {
    // `import Hello exposing [hello]` binds `Hello.hello`, and exposes it bare. The
    // module's top level becomes part of this program's, which is how the VM gets what
    // the tree-walker gets by evaluating each module into the shared global scope.
    let module = parse("Hello :: {}.{\n\thello = |name| \"Hello ${name}\"\n}\n\n{}");
    let app = parse("hello(\"World\")");
    let unit = vm::compile::Unit {
        modules: vec![vm::compile::Module {
            ast: &module,
            type_name: "Hello",
            exposed: vec!["hello"],
        }],
        app: &app,
        entry: None,
        ingested: Vec::new(),
    };
    let program = std::rc::Rc::new(vm::compile_unit(&unit).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "\"Hello World\"");

    // A name the module did not expose is not in scope bare.
    let unit = vm::compile::Unit {
        modules: vec![vm::compile::Module {
            ast: &module,
            type_name: "Hello",
            exposed: Vec::new(),
        }],
        app: &app,
        entry: None,
        ingested: Vec::new(),
    };
    assert!(vm::compile_unit(&unit).is_err(), "an unexposed name was in scope");
}

#[test]
fn an_ingested_file_is_a_top_level_string() {
    let app = parse("text.trim()");
    let unit = vm::compile::Unit {
        modules: Vec::new(),
        app: &app,
        entry: None,
        ingested: vec![("text", "  hello  ".to_string())],
    };
    let program = std::rc::Rc::new(vm::compile_unit(&unit).expect("compile failed"));
    assert_eq!(vm::run(&program).expect("vm failed").to_string(), "\"hello\"");
}
