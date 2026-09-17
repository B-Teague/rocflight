# rocflight

An interpreter for [Roc](https://www.roc-lang.org), written in Rust: a parser, a
bidirectional type checker, and a register VM in safe Rust.

Two goals, in this order:

1. **Full feature parity with the Roc compiler.** Not "mostly works" — every syntax
   feature is verified against the real `roc` binary, and a divergence is a bug.
2. **As close to bare metal as safe Rust reaches.** Every optimization is measured
   against a benchmark suite, and none of them is allowed to cost goal 1. It began as a
   tree-walker; a register VM replaced it, in safe Rust throughout — no `unsafe`, no
   loss of Rust's memory-safety guarantees.

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
different spelling, so `'a' + 1` is `98` — and, being a bare literal, it inspects as
`98.0` until something annotates it.

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
| Rust tests | **533** |
| Language examples | **19 of 19** comparable ones match `roc` byte for byte |
| `Builtin.roc` | **12 of 12** members parse; 1,443 definitions in Roc, 1,109 intrinsics in Rust |

```bash
tests/check_roc.sh --strict     # the 98 pairs — the definition of done
tests/check_examples.sh         # roc-lang.org's own examples
tests/check_builtin.sh --strict # the vendored Builtin.roc still parses
cargo test --quiet              # the Rust side
tests/bench.sh                  # performance, against a saved baseline
```

Nine of the 28 examples can't be compared at all: `roc` itself refuses them with this
compiler build, mostly platforms built for a different version. They're listed with
their reasons in `tests/check_examples.sh`. Every one of the other 19 matches byte for
byte, `Dict` and `Set` included — those run `Builtin.roc`'s own open-addressing table
rather than a Rust stand-in. See `BUILTIN_PLAN.md`.

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

It started as a tree-walker. Two rounds of representation fixes took it a long way, and
then a **register VM in safe Rust** replaced it — built alongside it for six phases,
differentially gated against it at every step, and switched over only once both engines
passed every gate on every file.

```
benchmark        tree-walker, first   tree-walker, tuned   register VM
calls                          80ms                 12ms           6ms
closure_capture               516ms                  3ms           2ms
closure_in_loop                75ms                 29ms           8ms
loop                           27ms                 22ms           9ms
matching                      224ms                 52ms          22ms
records                        32ms                 16ms           8ms
strings                        30ms                  5ms           5ms
```

`strings` and `list_ops` are unchanged by the VM, because they are bound by allocation
rather than by dispatch — which the plan predicted before any of it was written.
What is left of `iter_range` is the VM's ordinary per-instruction cost: four
instructions per element, at the same rate `loop` runs them.

Eight ceilings are gone rather than merely improved:

- List work is **linear**, not quadratic.
- Passing a list to a function is a **refcount bump**, not a copy. A register move
  used to deep-copy a `Vec`, so a loop that handed 8,000 elements to each iteration
  took four seconds; it takes six milliseconds, and `tests/bench/list_pass.roc` keeps
  it that way.
- Type-checking a block is **linear in its bindings**: every `let` used to rebuild the
  set of numeral-tainted variables and walk the whole environment, so 3,000 bindings
  cost 1.5 seconds. They cost 11 milliseconds.
- A chain of statements is walked in a **loop**, not a Rust frame per statement, in
  the parser, the checker and the compiler alike. 6,000 statements in one block
  overflowed the stack; 20,000 run in 48 milliseconds, and the next limit is the
  65,535 registers a frame may have. A file of 100,000 top-level declarations runs
  too, though top-level names are still found by a linear scan, so it takes seconds.
- `fold` and `map` on a list are **compiled into the frame** when the checker has
  proved the receiver is a list and no roc-defined method answers to the name. The
  builtin re-entered the VM from Rust once per element, with a fresh machine and an
  argument `Vec` each time; a compiled loop makes the callback an ordinary `Call`. And
  when the callback is a **literal lambda** whose body has no `return`, `break` or
  assignment, the body is compiled into the loop with its parameters bound to the
  loop's registers, so there is no call at all. `iter_range`, two million elements
  folded: 199ms to 90ms.
- Building a 40,000-character string peaks at 6 MB where it used to reach **710 MB**.
- Recursion is heap-allocated frames, so 500,000 levels run in 4 MB — the tree-walker
  exhausted a 256 MB reserved stack at 200,000 — and a **tail call reuses its frame**,
  so five million tail calls run in 2.8 MB.
- A `for` over a range never builds one, so `0..<10_000_000` allocates nothing.

Names are resolved once, at compile time: a local is a register, a captured variable an
index, a top-level name a slot, a top-level function a chunk id. Nothing compares a
string at run time. `Value` is **48 bytes** (it was 80), with a guard test to keep it
there, and the crate is `#![forbid(unsafe_code)]` — it contained exactly one `unsafe`, a
lifetime transmute around the AST, and removing the lifetime removed the need for it.

`OPTIMIZATION_PLAN.md` has the full method: every phase with its measurements, the
targets that were missed and by how much, three optimizations that were measured and
**rejected**, two gate corrections, and the four things the compiler refuses on purpose.

---

## Layout

```
src/parser/      the parser — the largest piece, and where most syntax lives
src/types/       bidirectional checker with let-polymorphism
src/vm/          the register VM: compiler, opcodes, machine
src/eval/        builtins, operators and runtime helpers
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
| `BUILTIN_PLAN.md` | how the vendored `Builtin.roc` is read, loaded and bounded |

---

## Known ceilings

Written down rather than hidden, because an interpreter that quietly disagrees with its
compiler is worse than one that says where it doesn't:

- A nominal's type ARGUMENTS are dropped: `Dict(Str, U64)` and `Dict(I64, Bool)` are one
  type here, so an element's type is still a variable. `Type` has no parameterised
  nominal.
- Nominals are erased, so the runtime tells them apart by SHAPE. A record with exactly an
  opaque nominal's fields inspects as `<opaque>` too, and a `Set` and a `Dict` are the
  same shape — only the checker separates those.
- The `Encoding` protocol's own members are Rust rather than roc's. JSON round-trips and
  a type's `encoder_for` runs, but another format would need the real thing.
- Iterators and `.iter()` are eager: `map` over a range still builds its output list.
- `where` constraints are read for the names they promise, not verified.
- A frame has at most 65,535 registers, and every `let` in a block takes one, so a
  single block of that many bindings is refused at compile time.
- A cyclic record type (`{ ..p, next: p }`) is refused by `roc` as anonymous recursion;
  the checker here reports nothing and the program runs. It used to crash the checker.

Full list, with the reasoning for each, in `IMPLEMENTATION_PHASES.md`; the builtin ones
in `BUILTIN_PLAN.md`.

---

## License

This interpreter is an independent project and is not affiliated with or endorsed by the
Roc Programming Language Foundation. The Roc language, its compiler, its documentation
and its examples are the work of the Roc authors, under the Universal Permissive License
1.0; the vendored examples under `tests/roc/examples/` retain that license and their
original copyright.
