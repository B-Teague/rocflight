# learning.md

How this interpreter works, what its words mean, and what you have to know before you
change it. `README.md` is the tour for a visitor; this is the map for whoever maintains
it next.

Read in this order: this file, then `PHASE_IMPLEMENTATION_GUIDE.md` (how a feature gets
added), then `TESTING_STRATEGY.md` (what "done" means), then whichever plan document
covers the area you are touching.

---

## 1. The one rule

**`roc` is the arbiter.** When rocflight and the real compiler disagree, rocflight is
wrong — always, including when rocflight's answer is more sensible. Every gate in this
repository is a comparison against `roc`'s own output, and nothing in it decides on its
own what correct means.

Two corollaries that have caused more bugs here than anything else:

- **Probe before implementing.** The language reference is partly aspirational
  (`list[i]` and `continue` are documented and rejected), and the reverse bites too —
  a form that looks broken is often just misused. Run it through `roc` first.
- **No quiet fallback.** Anything the compiler cannot lower is an `Err` naming the
  construct. There is no second engine to take over, because a program that silently
  runs through a different path is a program whose parity you no longer know.

The pinned compiler is `nightly-2026-09-03-62fcb65`, checked out at `roc-compiler/`
(untracked — it is a working copy of roc's own repo, not part of this one).

---

## 2. The pipeline

`rocflight file.roc` is one function: `run::run_file` (`src/run.rs`). Everything below
is a step inside it, in order. Set `ROCFLIGHT_TIME=1` on any run to see each step's
wall time; `ROCFLIGHT_CODE=1` dumps the compiled bytecode.

### Step 1 — desugar (`src/desugaring/`)

A **text-level** rewrite of the source before it is ever parsed: `!`-suffixed forms and
the shorthand that can be handled without structure. Type annotations are *preserved*,
not stripped — the desugared text has to be a real Roc program that passes `roc check`
on its own, because that is exactly what a golden pair's `.desugared.roc` file is.

Two forms are deliberately **not** here, because they need the expression structure and
therefore live in the parser: `??` (default value) and `?` (error propagation, which
moves the rest of the block into an `Ok` arm). `DESUGARING.md` has the per-rule detail,
including the list of things that *look* like sugar and are not (`foo!` — the `!` is
part of the identifier; `!foo` — a `Bool.not` call; `=>` — never `->`).

### Step 2 — parse (`src/parser/`, ~6k lines, the largest piece)

Hand-written recursive descent producing `ast::Expr`. It also collects a pile of
side-tables the later phases need and that cannot be recovered from the tree:
nominal declarations, nominal parameters, `where` methods, literal suffixes, overflowed
literals, field defaults, imports, ingests, the app's entry point.

Statement chains are walked in a **loop**, not a Rust frame per statement — here, in
the checker, and in the compiler alike. 6,000 statements in one block used to overflow
the stack.

### Step 2b — platforms and modules

- `platform::real::verify_app` resolves any real platform the app names, *before*
  checking, so a bad import is reported against the platform's sources.
- `builtin::needed_by(source)` reads which members of the vendored `Builtin.roc` this
  file actually needs, off the raw text. Only those load. This is never a command-line
  choice: `Builtin.roc` is the runtime, not an option.
- Local modules (`import Hello exposing [hello]`) are ordinary `.roc` files beside the
  importer, desugared and parsed the same way.
- Platform modules (`Stdout.roc` etc.) are loaded as the Roc they are. A member with a
  body is Roc; a member with only an annotation dispatches to the compiled host.

### Step 3 — type check (`src/types/checker.rs`)

Bidirectional checking — `synth` (infer) and `check` (verify against an expectation) —
with Hindley-Milner unification, let-polymorphism, and a substitution resolved at the
end.

The checker's *second* job is as important as the first: it hands the compiler facts
that let it emit better code and correct code. `integer_binops` (both operands proved
integer → `BinInt`), `dispatch_modules` (which module a method call resolves to),
`dec_literals` / `f32_literals` / `u128_literals` (which literal is which width),
`literal_conversions` (a literal standing for a nominal, via `from_numeral` /
`from_quote` / `from_interpolation`), `missing_fields`, `default_sites`,
`match_types`, `for_iter_calls`. Look at the construction of `compile::Unit` at the end
of `run.rs` for the full list — that assignment *is* the checker→compiler contract.

**Numeral polymorphism** is the subtlety that recurs. An unconstrained number literal is
fractional in Roc: `a = 7` prints `7.0`. So a literal synthesises to a *variable*, the
variable is recorded in `numeral_vars`, and whatever pins it decides the width; whatever
nothing pins defaults at the end. Most "wrong number type" bugs are a literal's variable
getting tied to the wrong thing.

### Step 4 — compile (`src/vm/compile.rs`)

AST → bytecode. Everything this pass does is work a tree-walker would redo on every
execution: which register a name lives in, which chunk a call goes to, which values a
closure captures, where a branch lands.

**Names are resolved here and never again.** A local is a register index, a captured
variable a capture index, a top-level value a slot, a top-level function a chunk id.
Nothing compares a string at run time — that is the whole reason this is faster than
walking the tree.

Then `vm::liveness` proves which reads are the last read of a value (so a move can
*take* rather than clone), and `vm::peephole` folds shapes like a jump-to-`IterNext`
into a single back-edge opcode.

The `expr` match is exhaustive over `Expr` **on purpose**: add an AST variant and this
file stops compiling until you lower it.

### Step 5 — run (`src/vm/mod.rs`)

A register VM. Four load-bearing properties:

1. **No `unsafe`** — the crate is `#![forbid(unsafe_code)]`. Register access is
   bounds-checked indexing, dispatch is a `match`, frames are indices. A wrong opcode is
   a panic with a message, never memory corruption.
2. **Names are compile-time** (above).
3. **Roc calls do not recurse in Rust.** `frames` is a `Vec`, so a Roc call costs ~32
   heap bytes, and "too deep" is a Roc error rather than a stack overflow. *The
   exception:* a builtin's callback (`xs.map(f)`) re-enters through `call_closure`,
   which does nest a Rust frame.
4. **A tail call reuses its frame**, so tail-recursive Roc runs in constant memory.

Builtins live in `src/eval/`. They take and return `Value`s and hold no interpreter
state, which is why they survived the tree-walker's removal unchanged.

---

## 3. Glossary

| Term | What it means here |
|---|---|
| **golden pair** | Two files per syntax feature: `x.roc` (sugared, *no* annotations) and `x.desugared.roc` (explicit types, no sugar). Four outputs must be byte-identical: `roc` and `rocflight`, on both files. The definition of done. |
| **eval backend** | rocflight wired into roc's *own* eval test runner as a fifth backend (alongside roc's interpreter, dev backend and wasm). `rocflight eval FILE` is its entry point. 1,953/1,953 as of 2026-09-19. |
| **intrinsic** | A `Builtin.roc` member with an annotation and **no body**. It must be implemented in Rust (`src/eval/`). The real compiler makes the same split in `BuiltinLowLevel.zig`. |
| **defined member** | A `Builtin.roc` member *with* a body. It is ordinary Roc, compiled into the program ahead of the app, and dispatch finds it like any `Type.method`. |
| **artifact** | `src/roc/Builtin.artifact` — `Builtin.roc` already parsed (and partly compiled), written by `gen-artifact` and `include_bytes!`d back in. Names in it are borrowed from the blob rather than interned. |
| **prefix / group** | The precompiled block of chunks and globals that `Builtin.roc` occupies at the front of every program. Same block for every program, so it can be compiled once. `artifact::Prefix`, `compile::Group`. |
| **chunk** | One compiled function: code, constants, register count, arity, name tables, pattern table, and a `spans` array mapping each instruction back to an AST node (that is how runtime errors get a line). |
| **slot / global** | A top-level binding that is not a function. Addressed by index. |
| **capture** | A value a closure copied out of the enclosing frame when it was made. A captured `var` is a shared `Value::Cell` instead, so assignment is seen by everyone. |
| **nominal** | `Name := backing` / `Name :: backing`. Distinct from every other nominal even with identical backing. **Erased at run time** — the VM tells them apart by *shape* (`NominalShape`), which is why `Set` and `Dict` look the same to the runtime. |
| **opaque nominal** | Declared with `::`. Inspects as `<opaque>` rather than showing its backing value. |
| **dispatch** | `a.method(b)`. Resolved at compile time when the checker names the receiver's module; otherwise at run time via `Program::methods_by_name`, ranked by shape fit and nominal depth. |
| **operator method** | Roc has no separate operator overloading: `a + b` *is* `a.plus(b)`, `==` is `is_eq`, `//` is `div_trunc_by`. A program that defines one gets `BinDispatch`; everything else gets plain `Bin`. |
| **hosted** | A platform function declared with a type and no body. Marshalled out to the platform's compiled host (`src/platform/hosted.rs`, `host/`). |
| **host library** | `host/` — the interpreter built as a static library that a platform's `main` links and calls `roc_main` on. The *only* crate with `unsafe`, by definition. |
| **numeral taint** | A type variable that came from a numeric literal (`numeral_vars`). It stays polymorphic until something pins it; unpinned, it defaults fractional. |
| **PEND** | A golden pair that is correctly written but that rocflight does not yet match. Non-fatal without `--strict`. |

---

## 4. The four data structures worth knowing cold

**`Value`** (`src/eval/value.rs`) — **48 bytes**, with a guard test. It is moved on every
binding, argument, list element and return, so its width is a tax on the whole
interpreter. Anything variable-length is behind an `Rc`: `Str(Rc<str>)`,
`List(Rc<Vec<..>>)`, `Record`, `Tuple`, `Tag(name, Rc<[Value]>)`, `Closure(Rc<..>)`.
That is not a micro-optimisation — it is why passing a list to a function is a refcount
bump instead of a copy, and why a loop that does it isn't quadratic. **If you add a
variant, box it and check the size test still passes.**

Numbers are not one type: `Int(i128)`, `U128`, `Float(f64)`, `F32(f32)`,
`Dec(i128)` (fixed-point, 18 places, *not* a float — that is the whole point of `Dec`),
plus `Simd`.

**`Type`** (`src/types/mod.rs`) — every name in it is an interned `&'static str`
(`memory::string_pool`), because a `Type` is cloned constantly and the checker has 135
clone sites. `Record` and `TagUnion` carry an `open` flag: open means "at least these",
which decides both unification and match exhaustiveness.

**`Op`** (`src/vm/mod.rs`) — the opcode set, grouped: loads/moves, arithmetic
(`Bin` / `BinInt` / `BinK` — the specialised forms exist because the checker proved
something), control flow and calls (`CallFn` known callee, `Call` value callee,
`TailCall` frame-reusing), aggregate construction, pattern **tests** (each jumps away on
*non*-match) then destructuring, loops (`IterNext` / `IterNextBack`), builtins and
dispatch, statements. Each variant's doc comment says why it exists; several exist
because of a specific measured cost, and deleting one to "simplify" reintroduces it.

**`Program`** (`src/vm/mod.rs`) — chunks, globals count, the top-level chunk, the
optional builtin `prelude` chunk, the entry point, the method tables, and the nominal
shape/depth tables that make run-time dispatch correct.

---

## 5. Repo map

```
src/desugaring/   text-level sugar, before parsing
src/parser/       recursive descent → AST, plus the side-tables later phases need
src/ast/          Expr, Pattern, node identity (NodeId → source offset → line)
src/types/        bidirectional checker, unification, the checker→compiler facts
src/vm/compile.rs AST → bytecode; liveness.rs and peephole.rs are its passes
src/vm/mod.rs     the opcodes, the machine, run-time dispatch
src/eval/         builtins, operators, Str.inspect, lazy iterators, Dec/F32 math, crypto
src/platform/     platform resolution, module loading, layout, ABI, marshalling, driver
src/builtin.rs    reading the vendored Builtin.roc: which members, bodied vs intrinsic
src/artifact.rs   the parsed-at-build-time Builtin.roc blob (read + write)
src/run.rs        the pipeline, as a library call
src/main.rs       the CLI
host/             the interpreter as a platform host library (the only unsafe)
build.rs          builtin member index, artifact staleness check, host lib embedding
tests/roc/        golden pairs (20 phases) + the 28 vendored examples
tests/bench/      benchmark programs + baseline.tsv
```

Plan documents: `IMPLEMENTATION_PHASES.md` (all 22 phases and their ceilings),
`PHASE_IMPLEMENTATION_GUIDE.md` (how to add a feature), `OPTIMIZATION_PLAN.md` (where
time goes, measured), `EVAL_PARITY_PLAN.md`, `BUILTIN_PLAN.md`, `PLATFORM_HOST_PLAN.md`,
`DESUGARING.md`, `TESTING_STRATEGY.md`.

---

## 6. Maintaining it

### The gates

```bash
cargo test --quiet              # the Rust side (~535 tests)
tests/check_eval.sh --strict    # roc's own eval tests, rocflight as a fifth backend
tests/check_roc.sh --strict     # the golden pairs — the definition of done
tests/check_examples.sh         # roc-lang.org's own examples, outside-in
tests/check_builtin.sh --strict # the vendored Builtin.roc still parses
tests/check_artifact.sh         # the artifact still regenerates byte-identically
tests/check_host.sh             # linked into basic-cli's real host, calling its effects
tests/bench.sh                  # performance against the saved baseline
tests/bench_compare.sh          # the same programs under roc's interpreter and dev backend
```

`check_eval.sh` and `check_roc.sh --strict` are the two that define parity. The rest
catch specific classes of rot.

### Adding a language feature

The full protocol is `PHASE_IMPLEMENTATION_GUIDE.md`. The shape of it:

1. **Establish ground truth with `roc`.** Write the snippet, run it, read the error.
   Do not guess from the langref.
2. **Write the golden pair and make it green under `roc` alone** — both files pass
   `roc check`, both `roc run` outputs agree, the sugared file carries *no* annotations.
   Only then touch Rust.
3. **Implement in pipeline order**: desugarer → parser/AST → checker → compiler → VM or
   builtin. Skipping a layer is how you get a feature that parses and mis-runs.
4. **Verify** with `check_roc.sh --strict`, then the eval suite, then `cargo test`.

The sugared file having no annotations is enforced, so it cannot quietly rot. Twelve
files keep one and each says why in a comment.

### Re-syncing the vendored `Builtin.roc`

When the pinned nightly moves:

```bash
cp roc-compiler/src/build/roc/Builtin.roc src/roc/Builtin.roc
cargo run --release --bin gen-artifact   # rebuild the parsed blob
tests/check_builtin.sh --strict
tests/check_artifact.sh
```

`build.rs` records an FNV-1a hash of the source in the artifact and **fails the build**
if they drift, with the regeneration command in the message. It also computes the
member byte-offset index (`builtin_index.rs`) so finding a member is a table lookup
rather than a scan of 23,555 lines. A skewed offset shows up as a member failing to
parse, which is what `check_builtin.sh --strict` catches.

Remember the contract: **bodied member = Roc, annotation-only member = your problem in
Rust.** If a sync adds an annotation-only member, it needs an intrinsic.

### Performance work

```bash
cargo build --release
tests/bench.sh --save   # BEFORE the change
# ... change ...
tests/bench.sh          # medians, vs. that baseline
```

Always the release build. Every benchmark carries its expected output and checks it, so
a change that is fast and wrong fails instead of looking like a win.

Two things to know before you measure:

- **`baseline.tsv` in the repo is not this machine's baseline.** Save your own before
  the change, or A/B against a pre-change binary.
- **roc folds pure calls with literal arguments at compile time.** A benchmark whose
  work does not depend on `args.len()` measures nothing.
- **The eval suite is not a benchmark** — its per-test times are roc's harness plus
  fork/exec, not rocflight's speed.

On a short program the VM is 1–4% of the time; the rest is the front end. That is what
the artifact and the on-demand member loading are about, and it is where the remaining
headroom is (`OPTIMIZATION_PLAN.md`).

### Invariants not to break

- `#![forbid(unsafe_code)]` in the interpreter crate. `host/` is the sole exception and
  documents every unsafe operation it performs.
- `size_of::<Value>() == 48`, guarded by a test.
- No run-time string comparison for *name resolution*. (Field and tag names are still
  compared by content at access sites — that is known and documented at `Chunk::names`.)
- The compiler's `expr` match stays exhaustive.
- Nothing is cached between runs: editing a `.roc` file always takes effect.
- No `--` options in the release binary. Every one that existed was a development aid,
  and a switch that changes how a program runs is a way to run it against something
  other than the real interpreter. Debug builds get `--show-ast`, `--show-desugared`,
  `--ast-only`, `--show-platforms`, `--builtins`; release rejects them as typos.
- Stderr is free (the harnesses discard it); stdout is the program's answer. That is why
  `ROCFLIGHT_TIME` and `dbg` go to stderr.

### Traps that have bitten before

- **A lone method answering everything.** A single `Try.is_eq` in scope used to answer
  every `==`, tuples included. Run-time dispatch filters candidates by nominal shape and
  only accepts an ambiguous set when one fits *exactly* — see `ranked_methods`.
- **Recursive `to_inspect`.** roc's unwrap changes the type so the inner `Str.inspect`
  finds no custom method; rocflight erases nominals so it finds the same one forever.
  The `INSPECTING` stack is what ends it.
- **A numeral defaulting to `Dec`** because its variable got tied to a placeholder. The
  checker's `declared_types` exists to stop a use-before-declaration from unifying
  through a shared sentinel.
- **A builtin callback re-entering the VM** nests a Rust frame. Deep `map`-inside-`map`
  is bounded by the Rust stack; lowering callback-taking builtins into bytecode is what
  removes that.
- **`Range` is not a `List`.** Building one as a list would print `[0, 1, 2]` where roc
  prints `<opaque>` and would wrongly satisfy a `List` parameter.

---

## 7. Known ceilings

Written down rather than hidden — an interpreter that quietly disagrees with its
compiler is worse than one that says where it doesn't. The current list is in
`README.md` ("Known ceilings"), with the reasoning for each in
`IMPLEMENTATION_PHASES.md` and the builtin ones in `BUILTIN_PLAN.md`. The headline
ones: nominal type *arguments* are dropped, nominals are erased so the runtime tells
them apart by shape, iterators are eager, `where` constraints are read but not verified,
and a frame has at most 65,535 registers.

When you hit a new one, add it to that list in the same breath as the workaround. A
ceiling in the README is a maintenance note; a ceiling only in someone's head is a bug
report waiting to be filed three phases later.
