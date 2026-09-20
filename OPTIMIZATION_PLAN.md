# Optimization plan

The interpreter is correct and the VM is done. This plan is about what is left, which
is **not the VM**: on the programs anyone actually runs, rocflight spends most of its
time re-deriving facts about a file that is compiled into the binary and never changes.

Rewritten 2026-09-19 against fresh measurements on this machine. The history of rounds
1–3 — the representation fixes, the six VM phases, the tree-walker's retirement and the
items retired with numbers — is in git: `git show afe5cf0:OPTIMIZATION_PLAN.md`. Nothing
here repeats it, and nothing here re-opens anything it closed.

---

## The hard requirement

**Every eval test keeps passing.** `tests/check_eval.sh` runs roc's own 1,953 eval
tests with rocflight as a fifth backend of roc's own harness, comparing `Str.inspect`
strings. That gate is the definition of "did not break anything", and it is checked
**per phase**, not once at the end.

Where the gate stands:

```
rocflight:      1953 of 1953 passed        (54.8s wall, 12 processes)
```

It read 1951 when this plan was written: `inspect: numeric default specialization
remains replaceable until constrained` and `trmc benchmark: NQueens (n=9)` were fixed
on the unmerged branch `worktree-eval-parity-last2` (210ab32), which is now merged.
**1953 is the number every phase below has to hold.** Anything that drops a test is
reverted, not explained.

### Two gates were already red, and still are

Both predate this plan and the merge — each confirmed by running it against `main`'s
`src/types/checker.rs` with everything else held still. Neither is a regression, and
neither is fixed here; they are written down so a later phase is not blamed for them.

- **`cargo test`**: `vm_test` fails `a_var_a_closure_captures_is_refused_rather_than_going_stale`
  and `what_the_vm_refuses_is_refused_deliberately`. Both assert the compiler REFUSES a
  `var` that a closure captures; it now compiles one through a shared cell (`MakeCell` /
  `CellGet`), so the tests outlived the refusal they were written for. Stale assertions,
  not a bug — but someone has to decide what they should assert instead.
- **`tests/check_examples.sh`**: `CustomInspect` and `GraphTraversal` fail, so it is 19
  matching examples and not the 20 the README claims. The script exits 0 because it is
  not `--strict`, which is how they went unnoticed.

Until those are settled, "the gates are green" means the other two are, and these two
read exactly as they read before the change. Compare, do not just look.

### The gates, per phase

```bash
cargo build --release
tests/check_eval.sh --strict      # 1953 of 1953 — the hard requirement
tests/check_roc.sh --strict       # 98 golden pairs
tests/check_examples.sh           # examples still byte-match roc
cargo test --quiet                # the Rust suite
tests/bench.sh                    # A/B, see below
```

`tests/bench/baseline.tsv` was saved on a different machine state and reads +50% on the
2–6ms benchmarks against an untouched tree. **Do not trust it.** Copy the pre-change
binary aside and A/B:

```bash
cp target/release/rocflight /tmp/before
# ... make the change, rebuild ...
ROCFLIGHT=/tmp/before tests/bench.sh --runs 9 --save
tests/bench.sh --runs 9
```

---

## Where the time actually goes

Measured 2026-09-19, release build, this machine, with `ROCFLIGHT_TIME=1` — the probe
Phase 0 landed. Reproduce any row of this section in one command.

### Three programs, end to end

| phase | `1 + 2` | `xs.map(\|x\| x * 2)` on 3 elements | `Dict.empty().insert(…).get(…)` |
|---|---|---|---|
| desugar | 18µs | 14µs | 16µs |
| parse | 43µs | 57µs | 56µs |
| `builtin::load` | 17µs | 14µs | **3541µs** |
| modules + platform | 10µs | 11µs | 11µs |
| type check | 33µs | **1404µs** | 458µs |
| build the compile unit | 17µs | 18µs | 22µs |
| compile to bytecode | 19µs | 26µs | 357µs |
| **run** | **18µs** | **22µs** | **68µs** |
| total in-process | 173µs | 1502µs | 4529µs |
| total wall | 852µs | 2366µs | 5329µs |

`/bin/true` on this machine is 523µs of wall, so ~160µs of the wall gap is rocflight's
own process startup and the rest is fork+exec.

**The VM is 1–4% of these programs.** Everything else is the front end, and almost all
of the front end is `Builtin.roc` — a 23,555-line file that ships inside the binary and
is byte-identical on every run.

### Inside that 3.5ms

```
slice (builtin::index, first call: scans all 700kB)     915µs
(low level) reachable  (word-set closure)               836µs
(low level) parse      (454 lines)                      660µs
Dict        parse      (674 lines)                      664µs
Set         parse      (230 lines)                      275µs
```

Then the type checker's `signatures_for` **parses `Dict` a second time** for its
annotations, 394µs, although `builtin::load` already holds them in `Loaded.signatures`.

And the `map` program never loads a member at all: its entire 1.4ms of "type check" is
`signatures_for("List")` parsing 1,676 lines of `Builtin.roc` to type one `.map`.

### What that costs the suite

| backend | mean | median | P95 | total over ~2000 runs |
|---|---|---|---|---|
| roc interpreter (in-process) | 3.8ms | 3.4ms | 6.9ms | 7.5s |
| roc dev backend | 3.6ms | 3.0ms | 6.8ms | 7.0s |
| **rocflight** (out of process) | **6.8ms** | **5.5ms** | **11.0ms** | **13.7s** |
| roc wasm | 23.9ms | 22.1ms | 36.3ms | 46.7s |

The median eval test costs 5.5ms; a trivial one costs 0.85ms. The ~4.6ms in between is
front-end work on a constant. Phases 1 and 2 are aimed squarely at it, and if they land
rocflight becomes the fastest backend in roc's own harness.

One outlier worth chasing separately: `issue 9796: multiple parser expects with forward
alias both finalize` takes **1297ms** in rocflight against 4.4ms in roc's interpreter.
That is not a 5ms-median problem, it is a single pathological path.

### And the run-time side

`tests/bench.sh`, release, today:

```
calls 6ms   closure_capture 3ms   closure_in_loop 11ms   iter_range 314ms
list_ops 3ms   list_pass 4ms   loop 11ms   matching 28ms   matching_tail 30ms
records 14ms   records_tail 11ms   strings 8ms
```

Micro-measurements behind those:

| program | time | per element |
|---|---|---|
| 2M-element inline `fold`, `I64` accumulator | 92ms | 46ns / 4 opcodes |
| the same with a `Dec` accumulator | 310ms | 155ns |
| `for x in 1..=2000000 { t = t + x }` | 104–174ms | ~70ns |
| `for _x in 1..=2000000 { t = t + 1 }` | 508–572ms | ~260ns |

Two findings there. The VM runs about **15ns per opcode**, which is 3–5× what a
register VM of this shape should cost. And the last two rows are the *same loop* with
the same answer: replacing a variable operand with a literal costs **3×**, which says
the specialised `BinInt` opcode is not firing where it obviously could.

---

## Phase 0 — make the front end measurable — **done**

No sampler works here: `perf` is not installed, `gprofng` recorded ~10% of samples, and
`valgrind` is absent. Every number above came from hand-placed `Instant::now()` probes
that were then thrown away. They are landed now.

```bash
ROCFLIGHT_TIME=1 ./target/release/rocflight eval main.roc
```

```
[time]                  desugar    0.027ms
[time]                    parse    0.062ms
[time]            slice + index    0.926ms      <- builtin::index, one scan of 700kB
[time]    (low level) reachable    0.797ms      <- a String per word, over 3,200 lines
[time]        (low level) parse    0.625ms
[time]               Dict parse    0.654ms
[time]                Set parse    0.369ms
[time]            builtin::load    3.411ms
[time]       modules + platform    0.011ms
[time]     signatures_for(Dict)    0.389ms      <- re-parses what `load` already has
[time]               type check    0.451ms
[time]           build the unit    0.021ms
[time]                  compile    0.344ms
[time]                      run    0.076ms      <- the VM, 2% of the program
```

`crate::timing()` and `crate::tick()` in `src/lib.rs` are the whole of it: `timing`
reads the variable once into a `OnceLock`, `tick` prints the DELTA since the previous
boundary and resets the clock. Nine call sites in `run::run_file`, three in
`builtin.rs` (the slice, the low-level pruning, each member's parse) and one on
`signatures_for`. Indented labels are the nested clock, so a member's lines sit under
the `builtin::load` total they add up to.

Two details worth keeping: `tick` takes `impl Display` so a caller interpolating a
member's name passes `format_args!` and allocates nothing when the timing is off, and
everything goes to **stderr**, which roc's eval harness discards — so this cannot
corrupt `rocflight eval`'s answer even left switched on.

Nothing was optimized here. It exists so Phases 1–3 are measured instead of argued
about, and so a regression six months from now is one environment variable away from a
diagnosis.

**Gate:** 1953 of 1953 eval, 99 of 99 pairs, 19 examples, benchmarks unmoved.

---

## Phase 1 — stop re-deriving constants at run time

`Builtin.roc` is `include_str!`'d. Its member boundaries, its low-level dependency
graph and its type signatures are **functions of a constant**, and `build.rs` already
exists. Every one of them is currently computed from scratch in every process.

Biggest measured payoff of anything in this document, and the lowest risk in it: the
output is byte-identical by construction, so the eval gate is a formality rather than a
worry.

### 1.1 — `builtin::index` moves to build time — saves ~915µs

`index()` is a `OnceLock` that scans all 700kB of `SOURCE` to find each member's byte
range. Once per process is still once per process, and it is 915µs of every program
that names a `Dict`.

Generate the table in `build.rs`: `static MEMBERS: &[(&str, usize, usize, bool)]`, in a
file `include!`d by `builtin.rs`. The run-time `index()` becomes a slice.

Keep the scanner — move it into `build.rs` and have a test assert the generated table
equals what it produces, so a re-sync of `Builtin.roc` cannot silently skew the offsets.

### 1.2 — `builtin::reachable` moves to build time — saves ~836µs

`reachable()` prunes the low-level section to what the loaded members mention, by a
word-set closure that allocates a `String` **per word** over ~3,200 lines. 836µs.

The closure's inputs are the member texts, which are constants, and the answer depends
only on *which members are loaded* — a set of at most a dozen. Precompute the low-level
block ranges per member in `build.rs` and union them at run time. A dozen ranges to
union is nanoseconds.

If the build-time version proves awkward, the cheap fallback is to fix the allocation
(`Vec<&str>` work-list, borrowing from `SOURCE`) — worth perhaps half of it. Prefer the
build-time table; the input is a constant, so run-time work here is waste by definition.

### 1.3 — `signatures_for` stops re-parsing — saves 0.4–1.4ms

Two separate wastes under one name:

- **When the member is loaded** (`Dict`, 394µs): `builtin::load` already parsed it and
  `Loaded.signatures` already holds every `Type.method` annotation. Thread it to the
  checker instead of parsing the text again. This was already on the table as "~0.9ms
  per Dict program" and nothing else in this document is cheaper to do.
- **When it is not loaded** (`List`, 1362µs; `Str` similarly): a three-element `.map`
  pays 1.4ms to type-parse 1,676 lines. Generate the signature tables for
  `TYPED_MEMBERS` in `build.rs` too. A signature is a `types::Type`, which is an
  ordinary Rust value — the generated file constructs them directly, no parser
  involved.

Doing the second half also removes the reason `TYPED_MEMBERS` is a list of four instead
of all ten: the comment says `Str` and `List` are excluded "for cost, not correctness",
and at build time there is no cost. **Widening it changes inference**, so widen it in a
separate commit with its own gate run.

### 1.4 — `needed_by` stops guessing — saves 0–3.5ms depending on the program

`needed_by` is four substring tests over the source. `source.contains("Dict")` loads
`Dict`, `Set` *and* the low-level section — 3.5ms — for a program that merely mentions
the word in a comment or an annotation. `contains("Try")` loads `Box`, and `Try` appears
in a great many eval tests' annotations.

After 1.1 the member index is a build-time table, which makes a precise answer cheap:
collect the qualified names the parsed AST actually references and load the members that
define them. The parse has already happened by then, so this is a walk over an AST that
is in cache, not another scan of the source.

Keep the closure — `Set(item) :: Dict(item, {})` genuinely needs `Dict` — but close over
what is *referenced*, not over what is *spelled somewhere in the file*.

### Targets

| program | now | after Phase 1 |
|---|---|---|
| `1 + 2` | 173µs | ~170µs (nothing to win) |
| 3-element `.map` | 1502µs | ~150µs |
| `Dict` insert/get | 4529µs | ~1800µs |
| eval suite, rocflight total | 13.7s | ~7s |

Labelled as targets. They land when `tests/bench.sh` and the suite's own performance
summary agree, and not before.

---

## Phase 2 — parse `Builtin.roc` once, not once per process

What Phase 1 leaves on a `Dict` program is ~1.6ms of parsing three members and ~360µs
of compiling them. Same observation as Phase 1 — the input is a constant — but the
output is an AST with node identity, interned `&'static str` and source offsets, so
this one is real work rather than a table.

Three ways, in increasing cost. **Measure 2a on one member before committing to
anything.**

- **2a — generate Rust that builds the AST.** `build.rs` emits constructor code per
  member. Fastest possible load (no parsing, no deserializing), but it inflates compile
  time and the generated file is large. Try it on `Set` (230 lines) alone and measure
  both the load win and the `cargo build` cost before going further.
- **2b — serialize a compact AST and deserialize on demand.** Smaller build impact, and
  the loader can be lazy per member. Costs a serializer, a deserializer and a format
  that has to stay in step with `ast::Expr` — the most code of the three.
- **2c — cache the compiled `Program` under `.rocflight/`, keyed by the hash of
  `Builtin.roc` and the member set.** Least code, but every process still pays a
  deserialize, and it puts correctness on a cache-invalidation rule. Last resort.

The awkward part either way: `NodeId` carries a source offset for error locations, and a
build-time AST has no offset into the *user's* file. That is fine — a runtime error
inside `Dict.insert` should point at the vendored source, which is what it does today —
but it has to be deliberate, because `ast::locate` silently returns `None` and the
error just loses its location.

**Only attempt this after Phase 1.** Phase 1 may take a `Dict` program to ~1.8ms, at
which point the remaining 1.6ms may be better spent on Phase 3, which helps user code
too. Decide with the numbers, not now.

---

## Phase 3 — the parser

~1.2–2.5µs per line, or ~100ns per byte. That is the floor under every module a user
imports, every platform module, `Builtin.roc` if Phase 2 is skipped, and the app itself.
No sampler on this machine can break it down, so this phase is a sequence of guarded
experiments, each measured on its own.

In the order they are worth trying:

- **`Parser::input: String` → `&'a str`.** `Parser::new` does `input.to_string()`,
  copying every source it is handed. One lifetime parameter, a copy removed, nothing
  semantic. Cheapest thing in the phase; may well be noise, which is worth knowing.
- **The string pool takes a global `Mutex` and a SipHash per identifier.**
  `string_pool::intern` locks `GLOBAL_POOL` for every name in every file. The process is
  single-threaded, so the lock is uncontended but not free, and the default hasher is
  the slow one. A thread-local pool with a fast hasher is a contained change.
- **One lexer pass instead of 269 `self.input[self.pos..].starts_with(…)` sites.** This
  is the big one and the risky one: the parser is 5,903 lines of char-level recursive
  descent, the desugarer feeds it, and 13 places save and restore `pos`. Behaviour must
  not move a millimetre — the golden-pair gate and 1,953 eval tests are what say so. Do
  it only if the two cheap items above show the throughput is in tokenizing rather than
  in allocation.
- **Allocation per AST node.** `Expr` boxes its children, so a member's parse is
  thousands of small allocations. An arena would fix it and would touch every file in
  the crate. Measure first: if swapping in a bump allocator behind a feature flag for
  one experiment does not move the parse time, the cost is elsewhere and this is a
  refactor for nothing.

**Do not start here.** Phase 1 is bigger, cheaper and safer, and after Phase 1 the
parser's share of a typical program is much easier to read.

---

## Phase 4 — VM per-op cost

~15ns per opcode. The inline `fold` loop is four opcodes per element and runs at 46ns,
which is consistent, so this is a per-op number and not a callback ceiling: callback
pooling was measured and removed, `fold`/`map` are already lowered into in-frame loops,
and a literal lambda is already inlined into them.

Ordered by measured evidence, strongest first.

### 4.1 — `BinInt` is not firing where it should — measured 3×

`for _x in 1..=2000000 { t = t + 1 }` is 508–572ms; the same loop with a variable
operand is 104–174ms, and both print the same answer. Same shape, same result, 3× the
time — so the specialised opcode is missing the literal-operand node.

Start at `TypeChecker::integer_binops`, which keeps a binop node only when
`self.apply(ty).is_integer()`. Find out whether the node's type is genuinely
unresolved, whether the accumulator's `var` type never unifies with the binop's, or
whether the literal defaults fractional and the operation really is `Dec` arithmetic.
**Confirm the mechanism before changing anything** — the fix differs completely between
those three, and a wrong guess here makes numbers wrong rather than slow.

### 4.2 — `Dec` arithmetic is ~155ns an operation

A 2M-element fold with a `Dec` accumulator is 310ms against 92ms for `I64`: ~110ns of
`Dec` overhead per add. `apply_binop` reaches `Dec` only after an `Int` probe, a
two-arm `U128` probe, `as_dec` on both operands and a `dec_binop` returning
`Option<Result<…>>`. `Dec` is not exotic in Roc — it is what an unconstrained
fractional literal becomes — so it deserves the same treatment `BinInt` gave integers:
a `BinDec` opcode where the checker proved both operands are `Dec`.

### 4.3 — register writes drop a 48-byte `Value`

Every `regs[base + dst] = …` runs drop glue on whatever was there, and `Value` is 48
bytes with `Rc` arms. That is a branch and possibly a refcount decrement on **every
instruction**, which is the most likely home of the 15ns. Two things to try, separately
and measured:

- a destination hint through `Compiler::expr`, so `Bin` writes where the result is
  wanted and the `Move` after it disappears (already identified as an `iter_range`
  item);
- shrinking `Value` below 48 bytes. `value_stays_narrow` guards the current width and
  the history says 32 was tried and lost to `i128`; the `Range` variant is the widest
  remaining inline payload and could be boxed. Measure across the whole suite — last
  time a width change helped `records` and moved nothing else.

### 4.4 — `DispatchMethod` resolves from scratch on every call

The opcode does a `module_for`, a `HashMap` lookup, and on a miss a scan of
`methods_by_name` with `NominalShape::is_exactly` per candidate, a depth computation and
a `holds_kinds` pass — per call, at a call site whose answer almost never changes. A
one-entry inline cache on the instruction (last receiver shape → chunk, invalidated by
nothing, because the program is immutable) is the standard fix and is contained.

### 4.5 — `GetField` compares field names

`Op::GetField` scans a `Vec<(&'static str, Value)>` comparing `&str`s. Resolving a field
to a slot needs the checker's record type at the access site; this was retired once as
not worth the threading. Revisit **only with a benchmark that shows it** —
`records` at 14ms for 40,000 update-and-read iterations is 350ns an iteration, which is
enough to be worth a look now that the plan has a measurement discipline for it.

### 4.6 — top-level names are found by linear scan

`Tops::global` / `Tops::func` and the checker's env are linear scans; 100k declarations
take 15s. Nothing anyone runs today hits this, and no gate covers it. A `HashMap` is
half an hour's work whenever a program shows up that cares. Left documented, not
scheduled.

---

## Phase 5 — lower the remaining callback builtins

`fold` and `map` compile into in-frame loops. Twenty-six other `call_function` sites in
`src/eval/mod.rs` do not: `each`, `filter`, `keep_if`, `drop_if`, `any`, `all`, `find`,
`walk`, `walk_until`, `keep_oks`, `sort_by`, `sort_with`, `update_at`. Each of those
re-enters the VM per element through `call_closure` — a fresh register file, an argument
`Vec` and a **Rust** frame apiece.

Two payoffs, one of them not about speed at all:

- the same per-element win `fold` and `map` got (199ms → 90ms on `iter_range`);
- **the Rust stack bound goes away.** `call_closure` is the one place where "calls do
  not recurse in Rust" stops holding, so `map` inside `map` inside `map` is bounded by
  the Rust stack with no Roc-level diagnostic. Lowering these removes the last such
  bound.

`Compiler::list_loop` is the pattern to extend, and its gates are the interesting part:
the receiver's module must be `List`/`Iter` per the checker, no roc-defined method may
answer the name, and a literal lambda is inlined only when `escapes()` finds no
`return`, `break` or `Assign`. Each new method needs the same care —
`filter`/`keep_if`'s predicate is `Bool`-typed and a non-`Bool` must still produce the
message it produces today, and `sort_with`'s comparator cannot be inlined into a loop at
all.

Do them **one method per commit**, with the eval gate on each. This is the phase most
likely to change an error message by accident, and an error message is what 1,953 tests
compare.

---

## Phase 6 — process startup

The binary is 13.5MB with fat LTO; a trivial program is 852µs of wall against 523µs for
`/bin/true`, so rocflight's own startup is ~160µs and fork+exec is the rest. Across the
eval suite that is ~1.2s of 13.7s — real, but a tenth of what Phase 1 is worth, and
most of it belongs to the operating system rather than to this code.

Worth one afternoon, after everything above:

- what the 160µs is (relocations? the `Lazy` statics? faulting in 700kB of
  `Builtin.roc` — which Phase 1 should already have stopped touching?);
- whether `codegen-units = 1` plus fat LTO is still paying for itself, measured on the
  suite rather than assumed.

---

## Not doing

- **A JIT.** The measured problem is a front end re-parsing a constant, not a slow inner
  loop. Ask again if Phases 1–5 land and something is still hot.
- **`unsafe`.** `#![forbid(unsafe_code)]` is load-bearing: a wrong opcode is a message,
  not memory corruption, and that is the whole argument for the VM being safe Rust.
  Nothing in this plan needs it.
- **Threads.** A run is a pipeline with a shared string pool, a thread-local node table
  and `RUNNING`. Parallelism would buy the eval suite nothing — roc's harness already
  runs 12 processes — and would cost the interpreter its simplicity.
- **A daemon or server mode for the eval harness.** It would be the single largest
  number in this document, because it amortizes the front end across 1,953 tests. It
  would also be a lie: the harness is the gate precisely because it runs rocflight the
  way a user does, once per program. Fix the front end instead — that helps the user
  too.
- **Caching the user's own parse between runs.** Editing a `.roc` file must take effect
  immediately; that invariant is worth more than the microseconds.

---

## Order, and why

1. ~~**Phase 0**~~ — done. Without it every later phase is an opinion.
2. **Phase 1**, the largest measured win in the document and the safest — its output is
   identical by construction. Halves the eval suite.
3. **Phase 4.1 and 4.2**, because a confirmed 3× on `t + 1` is embarrassing and the fix
   is local.
4. **Phase 5**, one method per commit — speed *and* a bound removed.
5. **Phase 4.3–4.5**, the general per-op work, each item measured on its own.
6. **Phase 2 or Phase 3**, whichever the post-Phase-1 numbers say is bigger. Not both
   at once.
7. **Phase 6**, or never.

Every phase, the same four gates, and the eval suite at 1953 of 1953. A phase that
cannot hold that number does not land, however good its benchmark looks.
