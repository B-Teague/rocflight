# rocflight

A tree-walking interpreter for [Roc](https://www.roc-lang.org), written in Rust.

Two goals, in this order:

1. **Full feature parity with the Roc compiler.** Not "mostly works" — every syntax
   feature is verified against the real `roc` binary, and a divergence is a bug.
2. **As close to bare metal as safe Rust reaches.** Every optimization is measured
   against a benchmark suite, and none of them is allowed to cost goal 1. The
   tree-walker got the representation fixes; the next round replaces it with a register
   VM in safe Rust — no `unsafe`, no loss of Rust's memory-safety guarantees.

```bash
cargo build --release
./target/release/rocflight path/to/main.roc
```

---

## First, the credit

**Roc is not ours.** This project interprets a language designed, built, argued over and
documented by other people, over years, for free.

Roc was created by [**Richard Feldman**](https://github.com/rtfeldman) and is built by a
community — the `authors` file in the compiler repo lists **172 names**, and the work is
stewarded by the [Roc Programming Language Foundation](https://www.roc-lang.org), a US
501(c)(3) nonprofit. It is released under the Universal Permissive License, © 2019
Richard Feldman and subsequent Roc authors.

Everything this interpreter knows about Roc, it learned from their work:

- The [**language reference**](https://github.com/roc-lang/roc/tree/main/docs/langref),
  which is what phase 21 was written against.
- The [**examples**](https://github.com/roc-lang/examples), all 28 of which are vendored
  here under `tests/roc/examples/` and used as the outside-in correctness gate. They
  found bugs no test of ours would have.
- The **compiler itself**, which is the arbiter for every test in this repo. When
  rocflight and `roc` disagree, `roc` is right.

If you find this project useful, the people to thank are theirs, not ours.
[Sponsor Roc](https://github.com/sponsors/roc-lang), or come say hello in
[their Zulip](https://roc.zulipchat.com) — they are, genuinely, very friendly about it.

> Roc is still pre-1.0. Its own README opens with *"Work in progress! Roc is not ready
> for a 0.1 release yet."* This interpreter chases a moving target on purpose, and is
> pinned to `nightly-2026-09-03-62fcb65`.

---

## Cool facts about Roc

Some from their docs, some learned the hard way while making this thing agree with the
compiler.

**It's named after a mythical bird.** The logo is an origami bird — an homage to Elm's
tangram logo. Per the FAQ, the name also won because it gives a three-letter file
extension and has "incredible potential for puns."

**Every operator is a method.** `a + b` *is* `a.plus(b)`. `-x` is `x.negate()`, `==` is
`is_eq`, `//` is `div_trunc_by`. Define `plus` on your type and it gets `+` — there is no
separate operator-overloading mechanism, because operators were never separate.

**The pipe binds tighter than everything.** `|>` binds *more* tightly than `+` and `*`,
which is the opposite of most languages that have it.

**There is no character type.** `'a'` is the number `97`. It is a number literal with a
different spelling, and `'a' + 1` is `98`.

**An unconstrained number literal is fractional.** `a = 7` then `a.to_str()` prints
`7.0`, not `7`. Annotate it and you get `7`. Type annotations are not always decoration.

**A range is not a list.** `Str.inspect(0..<3)` gives `<opaque>`, and you cannot pass one
where a `List` is wanted. It exists to be iterated.

**There is no null.** The FAQ quotes null's inventor calling it his "billion dollar
mistake." Roc has no null and no `Maybe` — errors are `Try` with explicit tags, so the
set of things that can go wrong is written in the type.

**An application has exactly one platform.** Not a framework, not a library set — the
platform author controls which primitives exist and how they are implemented, so a
platform can give a coherent experience for its whole domain. It is the most distinctive
idea in the language.

**The whole compiler runs in a browser.** No backend server. It compiles to machine code
or WebAssembly.

**The compiler is written in Zig now.** It used to be Rust. They rewrote it.

---

## Status

| | |
|---|---|
| Golden pairs | **98 / 98** across 20 phases |
| Rust tests | **436** |
| Language examples | **12 of 19** comparable ones match `roc` byte for byte |

```bash
tests/check_roc.sh --strict     # the 98 pairs — the definition of done
tests/check_examples.sh         # roc-lang.org's own examples
cargo test --quiet              # the Rust side
tests/bench.sh                  # performance, against a saved baseline
```

Nine of the 28 examples can't be compared at all: `roc` itself refuses them with this
compiler build, mostly platforms built for a different version. The seven that run but
don't match each need a subsystem this interpreter doesn't have — `Dict`, `Set`, the Json
package, the encoder framework, record-builder syntax, `Dec`'s precision, and roc's
fractional default for bare number literals. They're listed with their reasons in
`tests/check_examples.sh`, left failing rather than skipped, because the gap is ours.

### CLI Options

--show-desugared    Print the desugared source before running");
--show-ast          Print the AST and its inferred type, then run");
--ast-only          Print the AST and its inferred type, do not run");
--emit-desugared    Write the desugared source to .rocflight/cache/");
--show-platforms    Report each real platform the app resolves");
--test              Run the file's `expect`s and report, like `roc test`");
--clear-cache       Delete .rocflight/cache/desugared and exit");

### How parity is enforced

Every syntax feature gets a **golden pair**: a sugared file and a `.desugared.roc`
sibling with explicit types. Four outputs must be byte-identical — `roc` and `rocflight`,
on both files — and both files must build the same AST.

The sugared file carries **no type annotations at all**; types are inferred. Twelve files
keep one, each saying why in a comment: either the annotation *is* the syntax under test,
or `roc` refuses the file without it. The gate rejects any other annotation in a sugared
file, so this can't quietly rot.

The rule that makes it work: **probe the compiler before implementing.** The langref is
partly aspirational — `list[i]` and `continue` are documented but rejected by `roc` — and
the reverse also bites: a form that looks broken may just be misused. Guessing produced
more bugs in this project than anything else.

---

## Performance

The interpreter was profiled and optimized against `tests/bench.sh`, which reports
medians against a saved baseline and checks each benchmark's output, so a change that is
fast and wrong fails instead of looking like a win.

```
benchmark          before    after  speedup
calls                80ms     12ms     6.7x    function calls (naive fib)
closure_capture     516ms      3ms   172.0x    map+fold, lambda captures the list
closure_in_loop      75ms     30ms     2.5x    a closure built per iteration
matching            224ms     54ms     4.1x    tag construction and matching
records              32ms     16ms     2.0x    record update in a loop
strings              30ms      5ms     6.0x    string building
```

Three ceilings are gone rather than merely improved: list work is **linear** instead of
quadratic, building a 40,000-character string peaks at 6 MB where it used to reach
**710 MB**, and creating a closure no longer deep-copies its body. The wins came from
sharing scopes behind `Rc` instead of deep-copying them on every call, `Rc<str>` instead
of leaking every string, sharing the lambda body with the AST rather than cloning it,
and shrinking `Value` from 80 bytes to **32** so that every move in the interpreter is
cheaper.

The crate is `#![forbid(unsafe_code)]`. It contained exactly one `unsafe` — a lifetime
transmute around the AST — and removing the lifetime it worked around removed the need
for it.

A **register VM** is being built alongside the tree-walker, in safe Rust, and `--vm`
runs a program on it. It now covers the whole language: **both engines pass every gate this project has** —
98 golden pairs, 12 vendored examples, and all 196 suite files byte-identical.

On the same programs the VM is **2 to 8×** the tree-walker — `calls` 12ms against 6ms,
`matching` 52ms against 21ms, `loop` 22ms against 9ms, and a tail-recursive loop 154ms
against 24ms in 3 MB rather than 184 MB — while `strings` and `list_ops` are unchanged,
because those are bound by allocation rather than by dispatch. Its call frames are a `Vec`
rather than Rust stack frames, so it recurses 500,000 levels in 4 MB where the
tree-walker exhausts a 256 MB stack at 200,000 — and a tail call reuses its frame, so
five million tail calls run in 2.8 MB where the tree-walker cannot run them at all.

`cargo test --test vm_test` runs 56 programs on *both* engines and requires the same
answer, `tests/check_roc.sh --vm` and `VM_FLAG=--vm tests/check_examples.sh` put the VM
through the golden pairs and the examples, and `tests/vm_coverage.sh` checks all 196
suite files on both.

`OPTIMIZATION_PLAN.md` has the full method, one optimization that was measured and
**rejected** for not being worth its complexity, and the register-VM plan that replaces
the tree-walker — including the phase order, the falsifiable targets, and the stop
condition if the first phase misses them.

---

## Layout

```
src/parser/      the parser — the largest piece, and where most syntax lives
src/types/       bidirectional checker with let-polymorphism
src/eval/        the tree-walking evaluator
src/platform/    platform resolution, including real tarball loading
tests/roc/       98 golden pairs across 20 phases, plus the 28 vendored examples
tests/bench/     benchmark programs and the saved baseline
```

| Document | |
|---|---|
| `IMPLEMENTATION_PHASES.md` | all 22 phases, what each exposed, and the known ceilings |
| `PHASE_IMPLEMENTATION_GUIDE.md` | the golden-pair rule and how to add a feature |
| `OPTIMIZATION_PLAN.md` | performance method, results, and the register-VM plan |
| `TESTING_STRATEGY.md` | how the gates fit together |

---

## Known ceilings

Written down rather than hidden, because an interpreter that quietly disagrees with its
compiler is worse than one that says where it doesn't:

- An unconstrained number literal stays an integer; roc defaults it to fractional.
- `Dec` is `f64`; roc's fixed-point decimal carries more digits.
- Iterators and `.iter()` are eager.
- Custom `to_inspect` is resolved by trial, since values carry no nominal tag at runtime.
- `where` constraints are read for the names they promise, not verified.

Full list, with the reasoning for each, in `IMPLEMENTATION_PHASES.md`.

---

## License

This interpreter is an independent project and is not affiliated with or endorsed by the
Roc Programming Language Foundation. The Roc language, its compiler, its documentation
and its examples are the work of the Roc authors, under the Universal Permissive License
1.0; the vendored examples under `tests/roc/examples/` retain that license and their
original copyright.
