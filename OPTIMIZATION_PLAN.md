# Optimization plan

The interpreter is correct — 98 golden pairs, 439 Rust tests, 12 of 19 comparable
language examples byte-identical to `roc`. This is about making it fast **without
giving any of that back**.

The plan changed after the first round of work. Rounds 1–2 were representation fixes
(copying and leaking), and they are done. Round 3 replaces the tree-walker with a
**register VM in safe Rust** — no `unsafe`, no loss of Rust's memory-safety guarantees.
See [The new direction](#the-new-direction) for what changed and why.

**Both pre-VM items are done** ([round 3a](#round-3a-the-pre-vm-items-done)), and so are
**V0** ([round 3b](#round-3b-v0-the-machine-runs)), **V1**
([round 3c](#round-3c-v1-closures-and-the-clone-that-was-costing-40)) and **V2**
([round 3d](#round-3d-v2-aggregates-and-matching)) and **V3**
([round 3e](#round-3e-v3-var-loops-and-one-refusal)) and **V4**
([round 3f](#round-3f-v4-builtins-and-the-first-real-gate)) and **V5**
([round 3g](#round-3g-v5-both-engines-pass-every-gate)). **Every gate now passes on both
engines** — 98 golden pairs, 12 examples, 196 of 196 files byte-identical. What is left
is V6: make the VM the default, delete the tree-walker, delete `--vm`.

Everything in the history sections below is measured. Everything in the VM sections is
a **target**, labelled as such, and lands only when `tests/bench.sh` agrees.

---

## Measure first

```bash
cargo build --release
tests/bench.sh --save      # baseline BEFORE the change
# ... make the change ...
tests/bench.sh             # what it did
```

`tests/bench.sh` runs each program in `tests/bench/`, reports the **median** of five
wall times plus peak resident memory, and compares against the saved baseline. A change
of less than 3% is printed as `~` and means nothing — the machine is noisier than that.

Every benchmark carries its expected output on line 2 and the harness checks it on every
run, so a change that makes the interpreter fast and wrong fails here rather than
looking like a win.

### The rule for every change

A change lands only when all four are true:

```bash
tests/bench.sh                   # the benchmark it targets improved
tests/check_roc.sh --strict      # 98 pairs still green
tests/check_examples.sh          # 12 examples still match roc
cargo test --quiet               # 439 tests still pass
```

Optimizations break things quietly. The gates are what make "without losing feature
parity" checkable instead of hopeful. This applies to every VM phase below, per phase,
not once at the end.

---

## Rounds 1–2: done, and what they bought

Items 1, 2, 4, 5, 7 and 10 of the original plan are done, plus `Rc` for lambda
parameters. Every gate stayed green throughout.

```
benchmark          before    after  speedup     peak RSS   memory   roc*
calls                80ms     14ms     5.7x       2988kB     1.1x   35ms
closure_capture     516ms      3ms   172.0x       3748kB     1.8x   77ms
list_ops              4ms      3ms     1.3x       3332kB     1.2x   81ms
loop                 27ms     23ms     1.2x       2864kB     6.5x   97ms
matching            224ms     55ms     4.1x       2936kB     2.7x   79ms
records              32ms     17ms     1.9x       2880kB     2.2x   89ms
strings              30ms      5ms     6.0x       2956kB    18.9x  151ms
```

`*` roc's column is compile-and-run, not execution — it is what a user waits for on a
short script, not evidence that a tree-walker out-executes a compiler.

Two ceilings are gone rather than merely improved:

- List work is **linear**, not quadratic. `n=4000` was 519ms and `n=8000` 2032ms; they
  are now 4ms and 7ms, and `n=32000` is 21ms.
- A 5-million-iteration loop runs in **2.8 MB**, constant. Building a 40,000-character
  string peaks at 6.3 MB where it used to reach **710 MB**.

| # | Change | Effect |
|---|---|---|
| 4, 5 | `[profile.release]` with fat LTO; `opt-level = 1` for dev | 18-25% across the board, free |
| 1 | `Rc<RefCell<StackFrame>>` scopes | closure_capture **516ms → 4ms**, and quadratic → linear |
| 2 | `Value::Str(Rc<str>)` instead of leaking | strings **30ms → 6ms**, peak **55 MB → 3.6 MB** |
| 7 | `Rc<Expr>` lambda body | calls **-76%**, matching **-63%**, records **-20%** |
| — | `Rc<Vec<&str>>` lambda params | matching **-11%**, records **-10%** |
| 10 | Iterate a range without building it | 5M iterations now constant-memory |

### Measured and rejected

Skipping the per-call self-binding for non-recursive functions was priced by disabling
it: worth 1-2ms on `matching`, about 7-12%, and it needs an AST walk per lambda plus a
new field. Rejected — measured, not assumed. (The VM gets this for free: see
[Closures](#closures-captures-and-the-recursive-knot).)

---

## Round 3a: the pre-VM items, done

Two changes that stand on their own and de-risk the VM's numbers. Gates green
throughout: 98 pairs, 436 tests, 12 examples.

```
benchmark          before    after           what changed
calls                14ms     12ms   -14%
closure_in_loop      75ms     30ms   -60%    peak 66.7 MB -> 12.9 MB
records              17ms     16ms    -5%
loop                 23ms     22ms    -4%
matching             55ms     54ms      ~
closure_capture       3ms      3ms      ~     already at list_ops' level
list_ops              2ms      2ms      ~
strings               5ms      5ms      ~     allocation-bound, as predicted
```

### `Expr` lost its lifetime parameter, and the crate lost its `unsafe`

`Expr<'a>` propagated a lifetime that nothing used — every string in it was already
`&'static str`. That vestigial parameter was the whole reason for the lifetime transmute
in `Expr::Lambda`'s evaluation, and the transmute came with a **deep clone of the entire
lambda body, per closure created**.

`Expr::Lambda` now holds `Rc<Expr>` and `Rc<Vec<&'static str>>`, so evaluating it is two
refcount bumps. `src/lib.rs` and `src/main.rs` carry `#![forbid(unsafe_code)]`, which is
now a build gate rather than an aspiration.

No existing benchmark created closures in a loop, so this was unmeasurable — which is
why `tests/bench/closure_in_loop.roc` exists now. It builds a closure per iteration,
30,000 times. Old: **75ms, 66.7 MB peak**. New: **30ms, 12.9 MB**. The other benchmarks
create their closures once, which is why they show nothing.

### `Value`: 64 bytes → 32

`size_of::<Value>()` was 64, and `Value::Lambda` was why — four inline fields, 56 bytes,
32 of them an `Environment`. An enum is as wide as its widest arm, so every integer in
every list paid for it.

| Step | `size_of::<Value>()` | Effect |
|---|---|---|
| `Lambda(Rc<LambdaData>)` | 64 → **40** | `calls` -14%, `matching` -10%, `records` -11%, `list_ops` -33% |
| `Tag(&'static str, Rc<Vec<Value>>)` | 40 → **32** | `loop` -4%, everything else `~` |

A guard test in `src/eval/value.rs` asserts the 32 so a new inline field cannot quietly
put it back.

Boxing `Lambda` also deleted the hand-rebuilt closure in `apply`: tying the recursive
knot used to reconstruct the whole closure per call, carefully using the *captured*
environment so it would not nest a copy of the call frame at every recursive step. It is
now `Value::Lambda(l.clone())` — a refcount bump, and an `Rc` cannot make that mistake.

**Measured and rejected:** a shared empty payload for nullary tags (`Dot`,
`MissingField`), to skip the `Rc::new` allocation that a nullary tag pays for nothing.
`matching` constructs 60,000 of them and did not move — 52ms either way. Eight lines and
a `thread_local` for zero, so it went back out.

**Not done:** `Value::Builtin(String, usize)` is 32 bytes and holds an owned `String` for
a name that is always static. It is no longer the widest arm, so shrinking it would not
shrink `Value`; it would only save a `String` clone per builtin reference. Worth a
minute if a profile ever names it.

---

## Round 3b: V0, the machine runs

`src/vm/mod.rs` (the machine), `src/vm/compile.rs` (AST → bytecode), `--vm`, and
`tests/vm_test.rs` (the gate). Literals, locals, arithmetic, `if`, `let`, and direct
calls to top-level functions, recursion and mutual recursion included. Everything else
is a compile error naming the construct and the phase that will cover it.

### What it bought, on the subset it covers

No target was promised for V0 — it is the machinery. It is faster anyway:

```
fib(27), as a module both engines can run
  tree-walker   125ms
  VM             69ms     1.8x
```

And the ceiling the plan predicted is gone, which matters more than the 1.8×:

```
countdown(500_000), 500k levels of Roc recursion
  tree-walker   fatal runtime error: stack overflow, aborting   (with 256 MB of stack)
  VM            0, in 4.1 MB peak
```

The tree-walker aborts at 200,000 levels too. That abort is the one failure this
interpreter could produce with no diagnostic at all; on the VM, frames are a `Vec`, so
depth costs 24 bytes a level on the heap and the limit (`MAX_FRAMES`, a million) reports
itself as an ordinary Roc error.

### The gate

`tests/vm_test.rs` is **differential**: every program is run by both engines and the
answers must be identical. It asserts no value of its own devising. The tree-walker
agrees with `roc` on 98 golden pairs; if the VM agrees with the tree-walker, it agrees
with `roc`, and correctness is inherited instead of re-litigated.

Two of its cases are about not-diverging rather than about results:

- `unsupported_is_an_error_not_a_wrong_answer` — seven constructs from later phases must
  each be **refused** by the compiler. A silent fall-through to the tree-walker is how a
  two-engine interpreter stops being able to say which engine ran a program.
- `an_if_condition_must_be_a_bool` — both engines must refuse `if 1 { .. }` with the
  *same message*. Roc has no truthiness, and a VM that invented some would be a
  divergence no benchmark would ever catch.

All four repo gates stayed green: 98 pairs, 450 tests (436 + 14 new), 12 examples, and no
change to the tree-walker's benchmarks.

### Decisions worth recording

- **One implementation of the operators.** `Evaluator::apply_binop` is now an associated
  function that both engines call. Roc's arithmetic semantics — what `//` does to a
  negative number, how `Int` and `Frac` mix — exist once, so the two engines cannot
  drift on them. This is also why `Op::Bin` carries its `BinOp` instead of there being
  an `AddInt`/`AddFrac`/`AddAny` triple per operator: specialising is a *measured* step
  for later, and doing it now would mean writing Roc's arithmetic a second time before
  any benchmark could prove it pays.
- **`&&` and `||` do not short-circuit** — because the tree-walker's don't
  (`apply_binop` evaluates both sides). That is a divergence from `roc` worth fixing, but
  it is a change to the *language's* behaviour and does not belong in a phase whose gate
  is "identical to the tree-walker". Filed, not smuggled in.
- **No `spans` table yet.** The design called for `spans: Vec<u32>` per chunk to build
  error messages from the instruction pointer. It cannot be built: `Expr` carries no
  source positions at all, so there is nothing for a span to point at. Error *locations*
  are therefore blocked on adding spans to the AST, which is a parser change and its own
  piece of work. Error *messages* already match the tree-walker's, which is what the gate
  needs.
- **Globals are `Vec<Option<Value>>`.** A slot read before the top level filled it is
  "Used before it was defined" rather than a stale or default value. The order-independent
  top level still works: slots and chunk ids are handed out in a pass over all top-level
  bindings before any body is compiled, which is also what makes mutual recursion work.
- **A top-level function is a chunk id, not a value.** V0 has no function values at all,
  so `Op::CallFn` names its callee directly and a call allocates nothing — no argument
  `Vec`, no closure. First-class functions arrive with closures in V1, since they need
  the same machinery.
- **`size_of::<Op>()` is 12 bytes** (`CallFn` is the widest: two `u16`s, a `u32` and a
  `u16`). The plan said not to contort the encoding for size, and 12 is what fell out.
  Byte-packing operands is what an `unsafe` VM does for decode speed; this decode is a
  `match`.
- **The register allocator frees temporaries** by restoring a high-water mark, so a
  frame is sized by the deepest expression rather than by the number of nodes. `fib`'s
  frame is 4 registers.
- **Benchmark resolution.** `tests/bench.sh` reports whole milliseconds, so a 16ms
  benchmark quantises at ±6% and `records` reads as 16 or 17ms run to run. Measured at
  finer resolution it is 16.9ms and has not moved. Anything under ~20ms needs the finer
  measurement before a small delta means anything.

---

## Round 3c: V1, closures — and the clone that was costing 40%

Closures with captures, functions as values, dynamic calls, `return`, and tail calls.
Plus one measurement that turned out to matter more than any of them.

### What it bought

```
fib(27), as a module both engines can run
  tree-walker   126ms -> 121ms      (the borrow change below helps it too)
  VM             69ms ->  54ms
  ratio           1.8x ->  2.24x
```

```
5,000,000 tail calls
  tree-walker   fatal runtime error: stack overflow, aborting
  VM            5000000, in 2.8 MB peak — constant, not 5M frames
```

The tree-walker's own benchmarks moved too, from the same change: `closure_in_loop`
**-6%**, `loop` **-4%**, the rest flat.

### The finding: two `Value` clones per arithmetic instruction

V1 came in at 1.8× — the same as V0 — which was suspicious, since V1 removes two `Move`s
and a `Jump` per call. So before writing any of this up, one experiment: an `Int` fast
path inside the `Bin` opcode, four operators, thrown away afterwards.

```
fib(27), VM     69ms  ->  41ms      with a throwaway Int fast path
```

40% of the VM's time was `Value::clone` — two 32-byte clones per instruction, to add two
integers. `Op::Bin` cloned both operands because `apply_binop` took them by value.

The fast path was the wrong fix: it is a second implementation of Roc's arithmetic, and
the whole reason both engines call `apply_binop` is that there must only be one. The
right fix is smaller — **`apply_binop` takes `&Value`**. No arm in it ever needed to own
an operand: each one either copies a scalar out or builds a new value.

```
fib(27), VM     69ms  ->  54ms      operands borrowed, one implementation
```

That is most of the experiment's win with none of its duplication, and it is why the
tree-walker sped up as well. What is left of the gap — 54ms against the experiment's
41ms, about 24% — is the `(op, left, right)` tuple dispatch that runs before every
operation. **That is the measurement the plan's "specialise into `AddInt`/`AddFrac`"
item was waiting for**: an `AddInt` opcode skips the operand-shape dispatch entirely. It
now has a number attached and belongs to a later phase, once the checker's types are
threaded through the compiler.

**Measured and rejected:** `#[inline]` on `apply_binop`. 54ms either way — fat LTO had
already made the call site as good as it was going to get.

**Caught by measuring rather than by reasoning:** the first draft of V1 built an
`Rc<Closure>` on every direct call, so that `LoadCap` and `LoadSelf` had something to
read. That is a heap allocation per call, and it cost 4% (69ms → 72ms) before anything
else was measured. A top-level function captures nothing, so its closure is the same
object every time: `Chunk::bare` is built once at compile time and a direct call is now
a refcount bump.

### How closures work

Flat closure conversion, decided entirely at compile time:

- A lambda's free variables are found while compiling it, and each becomes a **capture
  index**. At run time `LoadCap` is an index into a `Vec` — nothing knows the variable
  ever had a name.
- A capture is one of three things in the frame that creates the closure: a register, one
  of *that* function's captures (so a grandparent's value threads down through every
  level in between), or the enclosing function itself.
- Captures are **by value**, which is what Roc's semantics say. A `var` that is captured
  *and* assigned needs a shared cell instead; `var` is V3, and so is that.
- A block-local function calls itself through `LoadSelf`, which reads the running
  closure out of the frame. The tree-walker rebuilds and rebinds a whole closure on every
  call to tie that knot, carefully using the environment as captured so it does not nest
  a copy of the call frame at each recursive step. `LoadSelf` cannot make that mistake,
  and cannot create the reference cycle that capturing yourself by value would.

A tail call reuses its frame: the arguments move down to where the parameters are and
execution restarts at the top of the callee. Roc's idiomatic accumulator loop is
therefore a loop, in constant memory, and does not count against `MAX_FRAMES`.

### A second gate correction, and a coverage census

The V1 row said its gate was "`check_roc.sh --strict` under `--vm` for phases 1–5". That
is impossible for the same reason V0's gate was: **all 196 golden pairs print with
`echo!`**, which is a builtin, so not one of them can run before V4. Counting beats
assuming, so there is now `tests/vm_coverage.sh` — it compiles every golden pair and
example with `--vm`, checks the ones that run against the tree-walker byte for byte, and
histograms what the rest were refused for:

```
ran identically     0
DIVERGED            0
refused           196

    84  a builtin (V4)
    48  a match (V2)
    14  a record (V2)
    14  a qualified name (V4)
    10  a var (V3)
     6  string interpolation (V4)
     6  a field access (V2)
     4  a record update (V2)
     4  a method call (V4)
     2  a tuple (V2)
     2  a tag (V2)
     2  a dbg (V5)
```

It is a report, not a gate — the gate stays `cargo test --test vm_test`, now 22
differential cases. But it is what decides phase order: `match` is 48 files and no
opinion moves that number.

### The target: missed, and the stop condition met

V1's target was `calls` at **≤4ms**, about 3.5× the tree-walker. The comparable
measurement is 2.24×. The plan's stop condition was *halving*, and that is met, so V2
proceeds — but the aspiration was optimistic and the reason is now measured rather than
guessed: operand-shape dispatch, worth about 24%, waiting on opcode specialisation.

### A deliberate divergence

`g(2)` where `g` holds `1` makes the tree-walker say **"Function 'g' not defined"**
(`src/eval/mod.rs:575`), which is wrong twice over: `g` is defined, and what is wrong
with it is its value. The VM says "Attempted to call a non-function value: 1" — which is
the message the tree-walker itself uses when the callee is not a bare name. Both engines
refuse the program; copying a misleading message into the engine that replaces the old
one at V6 would be the wrong kind of parity. `tests/vm_test.rs` pins this explicitly so
it stays a decision rather than becoming a drift.

---

## Round 3d: V2, aggregates and matching

Records, lists, tuples, tags; field access, record update, tuple indexing, optional
fields; and `match` lowered to compare-and-branch. 21 new opcodes, and every construct
V2 owns is gone from the coverage census.

### What it bought

Two new benchmarks, written so **both** engines can run them — a tail-recursive loop
instead of `var`/`for`, and the module's own value instead of `echo!`:

```
                tree-walker              VM              same program
matching_tail   155ms  183.8 MB     26ms  2.9 MB      6.0x, 63x less memory
records_tail     83ms  127.8 MB     10ms  2.9 MB      8.3x, 44x less memory
```

Most of the memory difference, and some of the time, is tail calls: written recursively
these cost the tree-walker 60,000 and 40,000 nested Rust frames. The **like-for-like
work** comparison is against the tree-walker's own formulation of the same benchmarks,
which uses `var` and `for`:

```
180,000 tag constructions and matches    tree-walker 53ms (var+for)   VM 26ms    2.0x
 40,000 record updates and field reads   tree-walker 16ms (var+for)   VM 10ms    1.6x
```

**Both targets missed.** V2 wanted `matching` ≤20ms and `records` ≤8ms. Neither is far
off, and the reasons are the two known-and-measured items rather than anything new:
operand-shape dispatch in `Bin` (about 24%, from
[round 3c](#round-3c-v1-closures-and-the-clone-that-was-costing-40)), and field lookup
comparing names instead of indexing a slot.

### `match` is compare-and-branch

Every arm's tests jump to the next alternative on failure, so a `match` costs only the
tests it runs. What the tree-walker does per arm it *tries* — not per arm that wins —
is: allocate a `Vec` for bindings, walk the pattern pushing `(name, value)` pairs into
it, push a scope, and bind each name into that scope by string.

Compiled, each of those is gone:

- A tag test is `TestTag { obj, name, n, to }` — one name comparison and an arity check.
- Destructuring is `GetPayload`/`GetIndex`/`GetElem`/`GetFieldOr`, each writing straight
  into the register the binding lives in. There is no bindings vector and no scope.
- A binding that gets written before a *later* test fails is simply dead: the register is
  reused by the next arm and the local is popped. That is the compiled equivalent of the
  tree-walker throwing away a half-filled bindings vector.
- In tail position each arm ends the function, so `area = |s| match s { ... }` has no
  result register and no jump to a common exit — it returns out of the arm that matched.

`A | B => body` compiles the body once per alternative. Sharing it would need the
alternatives to agree on which register each binding lives in, and they do not.

### One rule that had to be shared, and could not be reused

A literal pattern matches a value of the **same kind only**: `1` does not match `1.0`,
while `1 == 1.0` is `True`. So `TestLit` could not be lowered to the equality operator —
`values_equal` compares `Int` against `Float` numerically, and using it would have made
`match 1.0 { 1 => ... }` take the wrong arm.

`literal_pattern_matches(&Pattern, &Value)` is now `pub` in `src/eval/mod.rs` and both
engines call it: the tree-walker's three literal arms delegate to it, and the VM keeps
the literal patterns themselves in `Chunk::pats` so its opcode can. One implementation,
as with `apply_binop` — and the test for it is in `vm_test.rs`, where
`match 1.0 { 1 => "int" _ => "other" }` has to give `"other"` on both.

Field names are compared by content, not resolved to slots. Resolving a field to a slot
needs the record's type at the access site, which means threading the checker's types
through the compiler — a later phase, and the other half of the remaining gap above.

### Coverage: everything V2 owns is cleared

```
                       before V2          after V2
a match (V2)                  48                 —
a record (V2)                 14                 —
a field access (V2)            6                 —
a record update (V2)           4                 —
a tuple (V2)                   2                 —
a tag (V2)                     2                 —
a builtin (V4)                84               124
a qualified name (V4)         14                36
string interpolation (V4)      6                16
a var (V3)                    10                10
a method call (V4)             4                 8
a dbg (V5)                     2                 2
```

The V4 counts went *up* because a file is counted by the first construct the compiler
refuses: 76 files that used to stop at a `match` now get as far as their `echo!`. Still
0 of 196 golden pairs run end to end, and still for the one reason — printing is a
builtin.

`tests/bench.sh --vm` now exists, with its own baseline (`baseline.vm.tsv`, comparing
VM times against the tree-walker's saved ones would report every row as a change).
Under `--vm` it skips what the VM cannot compile yet, so it reports the two benchmarks
above today and more at each phase.

### Noted, not fixed

- **`size_of::<Op>()` is now 16 bytes**, up from 12: `GetFieldOr`'s three registers plus
  a `u32` jump target reach a 12-byte payload, and the discriminant rounds it to 16. The
  design said not to contort the encoding for size and to measure instead — `matching_tail`
  did not move, so this is recorded rather than acted on. Narrowing jump targets to `u16`
  would bring it back to 12 at the cost of a 65535-instruction limit per chunk.
- 33 differential cases now, covering literal/tag/tuple/record/list patterns, `..rest`
  in both records and lists, alternatives, guards, and the error wording for an
  unmatched value, a non-`Bool` guard, a missing field and an out-of-range tuple index.

---

## Round 3e: V3, `var`, loops, and one refusal

`var`, assignment, `for`, `while`, `break`, and ranges iterated without being built.
Two new opcodes, one removed, and every construct V3 owns is gone from the census.

### What it bought — and the first target met

The benchmarks now exist in a form **both engines run**, using the tree-walker's own
formulation (`var` plus `for`) rather than an approximation of it:

```
same program, both engines      tree-walker      VM      ratio
loop_mod                              21ms      8ms      2.6x     <- V3's target: <= 8ms
matching_mod                          54ms     23ms      2.3x
records_mod                           16ms      9ms      1.8x

tail-recursive versions, where the tree-walker has no frame reuse
matching_tail                        154ms     26ms      5.9x
records_tail                          82ms     11ms      7.5x
```

**V3's target is met**: `loop` at ≤8ms. It is also the first phase whose target was met
at all, and the reason is that a loop is where compiling names to registers pays most —
`total = total + i` is two register reads and a write, where the tree-walker walks its
scopes twice by name and then walks them again to assign.

These numbers also settle [V2's](#round-3d-v2-aggregates-and-matching) more precisely
than that round could: measured like-for-like, `matching` is 54ms → **23ms** and
`records` 16ms → **9ms**, against targets of ≤20ms and ≤8ms. Still just short, still for
the two known reasons — operand-shape dispatch and field-by-name.

### A range is computed, never built

`IterNext { dst, iter, idx, to }` is the whole of `for`: it reads the position from a
register, produces the next element or jumps past the loop, and advances. Over a range it
*computes* the element; over a list it reads one. So `for i in 0..<10_000_000` allocates
nothing — 320 MB of `Value`s that never exist — which is the same ceiling the tree-walker
removed by special-casing ranges inside `for`, expressed as an opcode instead. The loop's
own state is a register, so a loop allocates nothing at all.

`GuardFalse` is gone: `JumpFalse` now carries a `CondKind` saying whether it came from an
`if`, a `match` guard or a `while`. Roc words those three failures differently and the
tree-walker follows it, so the VM has to as well — but that is a difference in the
message, not in the opcode, and one op is better than three.

### The refusal

A `var` that a **closure captures** is refused, with a message that says why:

```
vm: `n` is a `var` captured by a closure, which needs a shared cell
    (V3 refuses it rather than copying it and going stale)
```

Captured by value it would be a snapshot, while the tree-walker's `Rc<RefCell>` frames
make it live — a silent disagreement, the worst kind. The fix is the shared cell the
design called for. **Nothing in roc's own suite does this**: all 10 `var` files use the
ordinary `var`-and-loop shape, where the variable is a register in the same frame and an
assignment is a register write. So it is refused rather than built for, with a
`ponytail:` comment on the upgrade path, and a test that pins the refusal — if that test
ever has to change, the cell is the change.

The same hazard in the other order — assigning a variable a closure has *already*
captured — is refused too, since the compiler learns about the capture first.

`break` is per-function: a `break` inside a lambda inside a loop is refused. The
tree-walker raises a break signal that propagates out through the call and ends the
outer loop, which is not behaviour worth copying into the engine that replaces it.

### Safe Rust earning its keep, concretely

The first run of `for` aborted with:

```
panicked at src/vm/compile.rs:306: internal error: entered unreachable code:
patched a IterNext { dst: 4, iter: 2, idx: 3, to: 4294967295 }, which is not a jump
```

`patch_to_here` did not know the new opcode carried a jump target, so the loop's exit
was never patched. In a VM that indexed raw memory this would have been a jump to
instruction 4,294,967,295. Here it is a message naming the opcode, the file and the line
— which is the entire argument for `#![forbid(unsafe_code)]`, and it cost one line to fix.

### Coverage

```
a var (V3)                    10                 —
a builtin (V4)               124               134
a qualified name (V4)         36                36
string interpolation (V4)     16                16
a method call (V4)             8                 8
a dbg (V5)                     2                 2
```

Everything left is V4 or V5. **V4 is the phase that matters**: builtins are why 0 of 196
golden pairs have run on the VM through three phases, and clearing them turns
`check_roc.sh --strict --vm` from an impossible gate into the real one.

---

## Round 3f: V4, builtins — and the first real gate

Strings and interpolation, the builtins, host effects, qualified names, and method
dispatch. This is the phase where the VM stops being measurable only on programs written
for it: **192 of 196 golden pairs and all 13 benchmarks now run on the VM with
byte-identical output**, and the 4 that do not are V5's (`dbg`, `crash`).

### What it bought

Every benchmark, on the real files — `echo!`, `I64.to_str`, `Ok({})` and all:

```
benchmark          tree-walker          VM        ratio
calls                     12ms         6ms         2.0x
closure_capture            3ms         2ms
closure_in_loop           29ms         9ms         3.2x    peak 15.0 MB -> 3.0 MB
list_ops                   2ms         2ms
loop                      22ms         9ms         2.4x
matching                  54ms        22ms         2.5x
records                   15ms         8ms         1.9x
strings                    5ms         5ms
matching_tail            154ms        26ms         5.9x
records_tail              83ms        11ms         7.5x
```

**V4's target — no regression on `strings` or `list_ops` — is met**: both are unchanged,
because both are allocation-bound, exactly as the plan predicted in
[What the VM will not fix](#what-the-vm-will-not-fix).

The earlier phases' targets can finally be judged on the benchmarks they named, rather
than on module-shaped approximations of them:

| | target | actual | |
|---|---|---|---|
| V1 | `calls` ≤ 4ms | **6ms** | missed |
| V2 | `matching` ≤ 20ms | **22ms** | just missed |
| V2 | `records` ≤ 8ms | **8ms** | met |
| V3 | `loop` ≤ 8ms | **9ms** | just missed |

Four predictions, one met, three within 10-50%. The gap has the same two measured causes
throughout: operand-shape dispatch in `Bin` (~24%, [round 3c](#round-3c-v1-closures-and-the-clone-that-was-costing-40))
and field lookup by name rather than by slot.

### Forty builtins, reused rather than rewritten

`List.map`, `Str.join_with`, the `Try` methods and everything else are the tree-walker's
implementations, called through a shim `Evaluator` the VM holds. The VM did not gain its
own copy of them, which is the only reason this phase was one round rather than five.

The problem that makes that interesting: `xs.map(|x| x * 2)` runs the tree-walker's
`List.map`, which calls `eval::apply` on the callback — and under the VM that callback is
a `Value::Closure`, which `apply` had never heard of. Solved by making the VM
**re-entrant**:

- `Program` became an `Rc`, and `globals` an `Rc<RefCell<..>>`, so a second machine over
  the same program sees the same top level.
- A thread-local holds the running program, and `eval::apply` grew one arm:
  `Value::Closure(c) => vm::call_closure(&c, args)`.

The cost is honest and worth stating: a builtin's callback nests a Rust frame, so
"[calls do not recurse in Rust](#the-register-file-and-frames)" holds for Roc calls but
**not across a builtin's callback boundary**. Deep `map`-inside-`map` nesting is bounded
by the Rust stack again. That bought forty builtins, and V6 can revisit it by lowering
the callback-taking builtins into bytecode.

Two things needed the same shim's *environment*, and both work because the VM binds
every `Type.method` chunk into it as a closure: `Str.inspect` looking for a nominal's own
`inspect`, and operator dispatch looking for `plus`/`is_eq`/…

### Operator methods, without taxing arithmetic

`a + b` IS `a.plus(b)` in roc, so every `+` has to consider a nominal method first — and
the tree-walker does exactly that, on every operator, forever. The VM instead decides at
compile time whether the program defines any operator method at all. Almost none do, so
almost every `+` compiles to plain `Bin` and never looks. A program that does overload
gets `BinDispatch` and pays for the lookup it asked for.

### Three bugs worth recording

The gate found all three, and each was a *wrong answer* rather than a refusal — which is
what the differential harness exists for.

1. **Builtins in tail position.** `tail()` had its own `Call` arm from V1 and never
   learned about builtins, so `List.map(xs, f)` at the end of a function compiled as a
   dynamic call *of a `Value::Builtin`*: "Attempted to call a non-function value" on 24
   pairs. A builtin call is not a tail call — it returns a value, which the function then
   returns.
2. **`Bool.True` is a value.** The tree-walker special-cases it before treating a
   qualified name as a builtin; the VM did not, and printed `<builtin Bool.True/1>`
   inside records on 6 pairs.
3. **`methods_named` only matched `Value::Lambda`.** A nominal's methods are `Closure`s
   under the VM, so `a + b` on a type defining `plus` silently fell through to the
   built-in operator — and `a == b` *appeared* to work, because structural equality
   happened to give the same answer on the test's data. One `matches!` arm.

A fourth was a refusal rather than a wrong answer: a helper declared *below* `main!` was
unreachable from inside it, because the parser wraps a file whose declarations are
followed by top-level `expect`s in a single `let _ = …`, which made every declaration a
block-local rather than a top-level one. The flattening now looks through that wrapper.
A genuinely block-local function referenced before its binding is still refused —
by-value capture cannot see a binding that does not exist yet — and that is the
remaining shape V5 has to handle for the examples.

### The gate

`tests/check_roc.sh --vm` and `tests/check_examples.sh` with `VM_FLAG=--vm` now exist, so
the two engines are checked by the same harnesses:

```
tests/check_roc.sh --strict            98 passed            tree-walker
tests/check_roc.sh --vm                96 passed, 2 pending VM (both pending are V5)
tests/check_roc.sh --strict --vm       96 passed, 2 failed  -> becomes V5's gate
tests/vm_coverage.sh                   192 ran identically, 0 diverged, 4 refused
cargo test --test vm_test              50 differential cases
```

V4's row claimed `--strict --vm` as its gate. Precisely, it clears every pair that does
not need V5, and `--strict --vm` is what V5 has to turn green — 2 pairs, `crash_untaken`
and `expect_and_dbg`. Under `VM_FLAG=--vm` the examples are 3 of 12, the rest blocked on
`expect`/`dbg`, which is the same V5 boundary.

---

## Round 3g: V5, both engines pass every gate

`expect`, `dbg`, `crash`, local modules, `ingest`, and `--test`. Three trivial opcodes
and three pieces of plumbing, and the VM reached parity:

```
                                        tree-walker        VM
tests/check_roc.sh --strict              98 passed      98 passed
tests/check_examples.sh                  12 passed      12 passed
tests/vm_coverage.sh                            —       196 ran identically, 0 refused
cargo test                                             493 passed
```

Both engines now pass **every gate this project has**, on the same files, with
byte-identical output.

### Where the VM stands

```
benchmark          tree-walker          VM        ratio
calls                     12ms         6ms         2.0x
closure_capture            3ms         2ms
closure_in_loop           28ms         9ms         3.1x     peak 13.7 MB -> 3.0 MB
list_ops                   2ms         2ms
loop                      22ms         9ms         2.4x
matching                  52ms        21ms         2.5x
records                   16ms         8ms         2.0x
strings                    5ms         5ms
matching_tail            154ms        24ms         6.4x     peak 183.8 MB -> 3.0 MB
records_tail              82ms        10ms         8.2x     peak 127.9 MB -> 2.9 MB
```

**2 to 8× on the same programs**, with `strings` and `list_ops` unchanged because they
are bound by allocation rather than dispatch — which is what
[What the VM will not fix](#what-the-vm-will-not-fix) predicted before any of it was
written.

### One implementation of everything shared

The pattern that made V4 and V5 one round each rather than five: anything both engines
need is extracted and shared, never copied. By now that is `apply_binop`,
`literal_pattern_matches`, `dispatch_builtin`, `interpolated`, `host_effect`,
`run_expect`, `run_dbg`, `crash_error`, and the forty builtins behind them. The two
engines cannot drift on what `//` does to a negative number, or on whether `1` matches
`1.0`, because there is only one answer in the tree.

`expect`'s tally is process-wide and both engines add to the same one, so `--test`
reports the same totals whichever ran — `report_tests` is one function now, not one per
engine.

### Local modules, compiled into one program

`import Hello exposing [hello]` compiles the module's top level into the **same**
`Program` as the app, ahead of it, with `hello` recorded as an alias for `Hello.hello`.
The tree-walker gets the same effect by evaluating each module into the shared global
scope before the app; this is that, decided at compile time. An alias is resolved last,
so a local binding of the same name still wins, and a name the module did not expose is
a compile error rather than a missing global at run time.

An ingested file is simpler than it looks: `import "data.txt" as text : Str` is a
top-level `Str` known before the program starts, so it is a global slot filled by a
constant.

### The catch-all is gone

The `expr` compiler's `other => Err("unsupported")` arm is **deleted**, because the
compiler reported it as unreachable: every `Expr` variant is now compiled. The match is
exhaustive on purpose — a variant added to the AST later is a compile error in the VM
backend rather than a program the VM quietly refuses at run time. `node_name`, which
existed only to name what was missing, is deleted with it.

### What the VM still refuses, on purpose

Four things, each because compiling it would mean *disagreeing* with the tree-walker
rather than failing loudly. `what_the_vm_refuses_is_refused_deliberately` is the
inventory, and each has its own test:

| Refused | Why |
|---|---|
| a `var` a closure captures | by value it would go stale; needs a shared cell |
| assigning a variable a closure captured | the same hazard, the other way round |
| `break` outside a loop in the same function | the tree-walker's break signal escapes through a call; not worth copying |
| `return` outside a function | the tree-walker unwinds to a call boundary that does not exist |

Nothing in roc's own suite does any of them. The first two are the shared cell the design
called for, and V6 is where to add it if a real program wants it.

### For V6

- **The callback boundary.** A builtin's callback re-enters the VM through
  `eval::apply`, which nests a Rust frame — so "calls do not recurse in Rust" holds for
  Roc calls but not across `xs.map(f)`. Lowering the callback-taking builtins into
  bytecode removes the last place the Rust stack bounds a Roc program.
- **The shim `Evaluator`.** The VM holds one so the tree-walker's builtins have
  something to be methods on. When the tree-walker goes, the builtins need a home that
  is not an `Evaluator` — a plain module of functions over `Value`.
- **`Op` is 16 bytes** and `Value` is 32. Both are measured, neither is contorted.
- The two optimizations with numbers attached and nothing else blocking them:
  **operand-shape dispatch** in `Bin` (~24% of `fib`) and **field access by slot**
  instead of by name.

---

## The new direction

The old plan said: *"A bytecode VM or a JIT. Every problem found so far is
representation — copying and leaking — not dispatch overhead."*

That was true then, and rounds 1–2 fixed the representation problems. What is left is
the sentence the old plan closed with:

> The remaining time is now spread fairly evenly across per-call work — `matching` does
> 180,000 calls in 55ms, about 300ns each, and `calls` runs at a similar rate. There is
> no single dominant cost left to remove.

No dominant cost left means the cost **is the structure**. 300ns per call is not one bad
line; it is the tree-walker's shape, and every item left in the old Tier 3 is a
different symptom of the same thing:

| Old item | Symptom | What it really is |
|---|---|---|
| 6 | `lookup` scans scopes comparing `&str` | names are resolved at **run** time, not compile time |
| 7 (done) | the lambda body was deep-cloned per call | the closure is **rebuilt** per call to tie the recursive knot |
| — (done) | the body was deep-cloned again per closure *creation* | the AST is not a runtime representation |
| 8 | `methods_named` does `format!` + a scan of every global | dispatch is rediscovered per call |
| 9 | error strings built eagerly | `Result` is the control-flow mechanism, `return`/`break` included |
| — | 256 MB of stack reserved (`src/main.rs:22`) | one Roc call costs **many Rust frames** |
| — (done) | one `unsafe` transmute in `Expr::Lambda`'s evaluation | the AST had a lifetime the runtime pretended was `'static` |

Every one of those disappears when the work moves to compile time and the runtime stops
being a recursive Rust function over `Expr`. Doing them one at a time means six
disruptive changes to the tree-walker for a few percent each; doing them together means
a register VM, where they are not optimizations but simply how it works.

**The goal is as close to bare metal as safe Rust reaches.** The constraint is not
negotiable: no `unsafe`, no loss of memory safety. What that costs is priced in
[The safety constraint](#the-safety-constraint) rather than hidden.

**Why register and not stack.** A register machine emits `Add r3, r1, r2` where a stack
machine emits `Push r1; Push r2; Add; Pop r3`. Roughly half the instructions dispatched
for the same arithmetic, and with `Value` at 32 bytes, the pushes and pops a register
machine avoids are 32-byte moves. Lua 5 made this switch and measured 20-40%; the whole
argument is in *The Implementation of Lua 5.0* (Ierusalimschy, de Figueiredo, Celes).

---

## The safety constraint

`#![forbid(unsafe_code)]` is in `src/lib.rs` and `src/main.rs`, and it is a build gate,
not a preference. The crate contained exactly one `unsafe` — a lifetime transmute around
the AST — and [round 3a](#round-3a-the-pre-vm-items-done) removed the lifetime it was
working around. Bytecode has none to transmute either.

What the constraint rules out, and what replaces it:

| Not available | Why it needs `unsafe` | Safe substitute |
|---|---|---|
| NaN-boxing / tagged-pointer `Value` | bit-punning a union | **shrink `Value`** by boxing fat variants — see [Value 64 → 32](#still-worth-doing-independent-of-the-vm) |
| Computed-goto / tail-call threaded dispatch | requires raw code addresses | `loop { match op }` — LLVM emits a jump table; costs one extra indirect branch per op |
| `get_unchecked` register access | no bounds check | slice-window borrow per frame, so the check is hoisted out of the hot path |
| Raw-pointer frames into the register file | aliasing | `base: u32` index into one `Vec<Value>` |
| `mem::transmute` for the AST lifetime | it is a lie | drop the lifetime parameter (it is already vestigial) |

**Honest expectation:** a safe-Rust register VM lands within roughly 1.5–2× of an
equivalent `unsafe` one. Against the *tree-walker* that is still a large multiple on
call- and lookup-heavy code, which is where all the remaining time is. If a number
below is missed, the answer is to profile, not to reach for `unsafe`.

**Bare metal, honestly.** A register VM is the last stop before emitting machine code.
Genuine bare metal means a JIT — Cranelift, which is a safe-Rust crate — and that is a
separate escalation with its own correctness surface (the executable-memory handoff is
`unsafe` inside `cranelift-jit` even if none is written here). Not now. The VM is the
prerequisite for it either way: a JIT compiles the same IR.

---

## The design

Five pieces. Nothing here needs a dependency that is not already in `Cargo.toml`.

### Chunks and instructions

One `Chunk` per function, produced once, immutable afterwards:

```rust
pub struct Chunk {
    code: Vec<Op>,             // flat, no nesting, no pointers
    consts: Vec<Value>,        // literals: Rc<str> strings are already cheap to clone
    n_regs: u16,               // frame size, known at compile time
    arity: u16,
    name: &'static str,        // errors and dbg only
    // spans: Vec<u32>,       // ip -> source offset — BLOCKED, see round 3b: `Expr`
                              // carries no source positions for a span to point at
}
```

`Op` is a `#[repr(u8)]`-ish enum of fixed-size variants — three-address, `u16`
registers:

```rust
type Reg = u16;

enum Op {
    // movement
    LoadK    { dst: Reg, k: u32 },
    Move     { dst: Reg, src: Reg },
    LoadCap  { dst: Reg, idx: u16 },        // captured value
    LoadGlob { dst: Reg, idx: u32 },        // top-level slot, resolved at compile time
    LoadSelf { dst: Reg },                  // the running closure — ties recursion

    // arithmetic: specialised when the checker proved the operand types
    AddInt   { dst: Reg, a: Reg, b: Reg },
    AddFrac  { dst: Reg, a: Reg, b: Reg },
    AddAny   { dst: Reg, a: Reg, b: Reg },  // generic; may dispatch to a `plus` method
    // ... one triple per BinOp, 14 of them

    // control — `return` and `break` are jumps, not Err values
    Jump     { to: u32 },
    JumpFalse{ cond: Reg, to: u32 },
    Ret      { src: Reg },

    // calls
    Closure  { dst: Reg, chunk: u32, caps: Reg, n: u16 },   // caps in regs[caps..caps+n]
    Call     { dst: Reg, func: Reg, base: Reg, argc: u16 },
    TailCall { func: Reg, base: Reg, argc: u16 },
    CallBuiltin { dst: Reg, id: u16, base: Reg, argc: u16 },

    // aggregates
    MakeList { dst: Reg, base: Reg, n: u16 },
    MakeRec  { dst: Reg, shape: u32, base: Reg },
    GetField { dst: Reg, obj: Reg, slot: u16 },   // slot, not a name, when the shape is known
    MakeTag  { dst: Reg, tag: u32, base: Reg, n: u16 },
    TagIs    { obj: Reg, tag: u32, to: u32 },     // one compare-and-branch per match arm
}
```

Arguments go in consecutive registers, so `Call` needs one `base` rather than a
`Vec<Value>` per call. The `Vec<Value>` allocated per call today goes away.

`Value` is 32 bytes (it was 64 — see [Round 3a](#round-3a-the-pre-vm-items-done)), which
is what makes a register file of them affordable.

**Where `Op`'s size matters:** keep it at 8 bytes if it falls out naturally, but do not
contort the encoding for it. Byte-packed operands are what an `unsafe` VM does for
decode speed; the safe version pays a little cache for a decode that is a `match`.
Measure `size_of::<Op>()` and move on.

### The register file and frames

One contiguous `Vec<Value>` for the whole VM. A frame is a window into it:

```rust
struct Frame {
    chunk: u32,
    ip: u32,
    base: u32,                 // regs[base .. base + chunk.n_regs]
    dst: Reg,                  // where the caller wants the result
    caps: Rc<[Value]>,         // this closure's captures
}

pub struct Vm {
    regs: Vec<Value>,
    frames: Vec<Frame>,
    chunks: Vec<Chunk>,
    globals: Vec<Value>,       // the top level, by slot
}
```

Two things follow, and both are worth more than the speed:

- **Calls are a `Vec` push, not Rust recursion.** The 256 MB stack reservation in
  `src/main.rs:22` — there because a tree-walker burns many Rust frames per Roc call and
  died at ~200 levels of Roc recursion — is deleted. Recursion depth becomes a heap
  `Vec` that grows, with a configurable limit that produces a Roc-level error instead of
  a stack overflow. A stack overflow in safe Rust is still an abort with no diagnostic;
  this removes the last way this interpreter can die without saying why.
- **Deep recursion stops being a memory risk.** `frames` is 32ish bytes per level
  instead of kilobytes of Rust stack.

Register access is `self.regs[base + r as usize]` — bounds-checked. In the hot loop,
borrow the window once per instruction batch (`let f = &mut self.regs[base..base+n]`)
and re-borrow after any op that can move the frame stack, so the check is hoisted rather
than paid per operand. A register index out of a chunk's range is a **compiler** bug:
validate `max_reg < n_regs` once when a chunk is built, and the runtime check is then
only a branch predictor's problem, not a correctness one.

### Names: resolved once, at compile time

This is where the win actually comes from, and it is worth being concrete about how big
it is. Today `Environment::lookup` walks every scope and, within each, every binding,
comparing `&str` — for every identifier, every time. `fib(n)` does that for `n` and for
`fib` on each of 28,657 calls.

The compiler assigns every name to one of four things, and none of them involves a
string at run time:

| Kind | Resolved to | Op |
|---|---|---|
| parameter or local | a register in this frame | operand, no op at all |
| captured variable | an index into `caps` | `LoadCap` |
| top-level name | a slot in `globals` | `LoadGlob` |
| the running function itself | the frame's own closure | `LoadSelf` |

The top level stays order-independent — that property is why `top` is shared behind an
`Rc` today. It survives because slot *allocation* happens in a pass over all top-level
declarations before any body is compiled, so a body can name a function declared below
it. Forward references resolve to a slot that is filled later; a slot read before it is
filled is the same error the tree-walker gives today, raised from the same place.

This is old item 6, done properly rather than as the "intern to `u32` and compare
integers" half-measure. Interning makes the scan faster; slots delete the scan.

### Closures, captures, and the recursive knot

Compile-time free-variable analysis per lambda gives an exact capture list, so
`Closure` copies *n* values into an `Rc<[Value]>` once, at closure creation. No
environment, no `RefCell` chain, no scope vector.

Two cases need care, and both are decided at compile time:

- **A captured `var` that is assigned.** Only `var` can be reassigned in Roc, so the set
  is small and statically known. Such a variable is boxed — `Rc<RefCell<Value>>` in the
  capture slot, and `LoadCap`/`StoreCap` go through the cell. Everything else is
  captured by value, which is what Roc's semantics say anyway. The behaviour to preserve
  is in `tests/roc/16_loops/`; a shared frame makes it work today by accident, and a box
  makes it work on purpose.
- **Self-reference.** `apply` currently rebuilds the whole closure on every call to tie
  the recursive knot (`src/eval/mod.rs:1686`). A function does not capture itself:
  `LoadSelf` reads the closure out of the running frame. That deletes the per-call
  rebuild, deletes the `Option<&str> self_name` field, and avoids the `Rc` cycle that
  capturing yourself by value would leak. It is also the optimization the old plan
  measured and rejected as not worth its complexity — here it costs nothing, because the
  information is already in the frame.

### Dispatch, builtins, and the type checker

- **Builtins** stay ordinary Rust functions. `CallBuiltin` takes a `u16` id into a
  static table, resolved at compile time, so `format!(".{}", name)` and the
  `ends_with` scan over every global (old item 8) are gone.
- **Methods.** The checker already knows the receiver's type at most call sites; where
  it does, compile a direct `Call` to the resolved chunk. Where it genuinely does not,
  fall back to a table built once. The current ambiguity ceiling in
  `Environment::methods_named` — two nominals with the same method name — is a
  *compile-time* error to report properly once the checker records the method per call
  site, which the doc comment there already says it knows how to do.
- **Arithmetic specialisation.** `AddInt` when the checker proved both sides `Int`,
  `AddAny` otherwise. Worth about **24%** on `fib`, measured in round 3c by pricing a
  throwaway fast path; the cheaper half of that win (borrowing the operands instead of
  cloning them) is already taken. Overflow and division semantics stay exactly as
  `apply_binop` has them — matching `roc` is goal 1 and specialisation must not
  quietly change a result. Any `*Int` op whose operands are not both `Int` at run time
  is a compiler bug, not a fallback path: debug-assert it, and let the gates catch it.

---

## Landing order

Six phases. Each one is gated, benchmarked, and committed on its own. Unsupported AST
nodes are a **hard compile error** in the VM backend — never a silent fall-through to
the tree-walker — so coverage is always visible and a gate failure is never "which
engine ran this?".

| Phase | Scope | Gate | Target |
|---|---|---|---|
| ~~**V0**~~ **done** | `Chunk`, `Op`, `Vm`, the compiler skeleton; literals, locals, arithmetic, `if`, `let`, direct calls to top-level functions. Behind `--vm`. | `cargo test --test vm_test` — both engines, same answers | none promised; got **1.8×** on `fib` |
| ~~**V1**~~ **done** | closures, captures, functions as values, dynamic calls, `TailCall`, `return` as a jump | `cargo test --test vm_test` — 22 differential cases | missed `≤4ms`; got **2.24×**, and the stop condition |
| ~~**V2**~~ **done** | records, lists, tuples, tags; pattern matching as compare-and-branch; field access **by name**, not slot | `vm_test` (33 cases), and `vm_coverage.sh` clear of every V2 construct | missed both; got **2.0×** on matching, **1.6×** on records |
| ~~**V3**~~ **done** | `var`, `for`, `while`, `break`, ranges (lazily, as now) | `vm_test` (42 cases), `vm_coverage.sh` clear of `var` | **met**: `loop` 21ms → **8ms** |
| ~~**V4**~~ **done** | strings and interpolation, builtins, host effects, qualified names, method dispatch | `check_roc.sh --vm`: **96 of 98**, the 2 pending being V5's; `vm_test` (50 cases) | **met**: every benchmark runs, `strings` and `list_ops` unchanged |
| ~~**V5**~~ **done** | `expect`, `dbg`, `crash`, local modules, `ingest`, `--test` under `--vm` | **met**: `check_roc.sh --strict --vm` 98 of 98, `check_examples.sh` with `VM_FLAG=--vm` 12 of 12, `vm_coverage.sh` 196 identical and 0 refused | — |
| **V6** | **flip the default, delete the tree-walker, delete `--vm`** | all four gates | drop the 256 MB stack reservation |

**V6 is not optional.** Two engines for the same language is the expensive failure mode:
every feature gets built twice, every bug gets diagnosed twice, and the gates stop
telling you which one is wrong. The `--vm` flag exists to make V0–V5 committable, and it
is deleted in the same commit that makes the VM the default.

**Two gate corrections, both the same mistake.** V0's row said "every `tests/bench/`
program runs under `--vm`" and V1's said "`check_roc.sh --strict` under `--vm`". Neither
was checkable, for one reason: **every benchmark and every golden pair prints**, and
printing is `echo!`, a builtin. Nothing outside `tests/vm_test.rs` can run on the VM
until V4. The gate before then is the differential test; `tests/vm_coverage.sh` counts
coverage in the meantime, and the whole-suite gates move to V4 where they first mean
something.

**The stop condition:** if V1 does not at least halve `calls`, stop and profile before
writing V2. The whole argument for the VM is that per-call work is structural; if
removing the structure does not move the per-call benchmark, the argument is wrong and
the rest of the phases are not owed.

### What the VM will not fix

Stated up front so the results are not a surprise:

- **`strings` and `list_ops` are allocation-bound**, not dispatch-bound. Their time is
  in `Rc<str>` concatenation and `Vec<Value>` growth, and a VM moves neither. `strings`
  is still **18.9×** roc's memory; that is old item 11 (lazy iterators), not this.
- **Startup.** Parse and check are 1-3ms and the VM adds a compile pass. Expect startup
  to get *slightly worse* — worth it on anything that runs longer than a millisecond,
  and the reason [the caches](#in-memory-caches) are worth their ten lines.
- **`closure_capture` at 3ms** is already at `list_ops`' level. There is nothing there.

---

## In-memory caches

Nothing is cached on disk. Not source, not AST, not bytecode.

The reason is a correctness property worth more than the milliseconds: **edit a `.roc`
file and it takes effect.** A disk cache buys 1-3ms of parse time and owes staleness
detection, a cache key that has to include every input that can change a result (the
file, its imports, the platform, the interpreter's own version), and a class of bug where
the interpreter disagrees with the file on screen. `.rocflight/cache/desugared/` stays
what it is: an opt-in, write-only dump for inspection under `--emit-desugared`, never
read back.

In memory, within one process, caching is free of all of that — the process cannot
observe a mid-run edit anyway:

```rust
// src/main.rs — module loading
asts:   HashMap<PathBuf, Rc<Expr>>,   // canonical path -> parsed AST
chunks: HashMap<PathBuf, u32>,        // canonical path -> compiled chunk id
```

Keyed on the **canonicalized** path, so `./Hello.roc` and `Hello.roc` are one entry.

**Honest scoping, because this is the part that is tempting to over-build:** today it
saves nothing. `src/main.rs:204` loads the app's direct `local_modules` in a flat list,
each exactly once — no recursion, so no diamond, so no module is parsed twice. The cache
becomes real the moment either of these is true, and not before:

1. **Modules import modules.** A diamond (`A` imports `B` and `C`, both import `D`)
   parses `D` twice without it. That is where it earns its keep.
2. **Anything long-running** — a REPL, a watch mode, a test runner that evaluates many
   files in one process. `PLATFORM_CACHE` in `src/platform/cache.rs` is already exactly
   this pattern for platforms; the AST cache is the same idea, ten lines.

The **chunk cache is the one the VM makes load-bearing**, and unlike the AST cache it
pays immediately: compile each function once per process, then call it as many times as
the program likes. That is not really a cache, it is the compile step, and it is already
in the design above.

Write the AST cache when condition 1 or 2 lands. Writing it now is a `HashMap` that is
never hit twice.

---

## Still worth doing, independent of the VM

### ~~`Value`: 64 bytes → 32~~ — done, see [Round 3a](#round-3a-the-pre-vm-items-done)

`Value` is 32 bytes and a guard test in `src/eval/value.rs` keeps it there. This was the
safe answer to NaN-boxing: half the moves of the 64-byte original, no `unsafe`.

### ~~Drop `Expr`'s lifetime parameter~~ — done, see [Round 3a](#round-3a-the-pre-vm-items-done)

Gone, along with the crate's only `unsafe` and the per-closure deep clone of the lambda
body. `#![forbid(unsafe_code)]` is in now rather than at V6.

### Old items that the VM subsumes

Do **not** do these separately; they are phases V0–V4 by another name, and each is a
disruptive change to a tree-walker that is about to be deleted:

- **Item 6** (slot-indexed variables) → [Names](#names-resolved-once-at-compile-time).
- **Item 8** (method dispatch allocates and scans) → `CallBuiltin` + a compile-time
  method table.
- **Item 9** (eager error strings) → an error message is built when an error actually
  happens, so there is nothing eager left to fix. Error *locations* additionally need a
  span table, which needs spans in `Expr` first — see [round 3b](#round-3b-v0-the-machine-runs).
- **Item 3** (`Rc<Vec<Value>>` lists) → measure again after V2. Item 1 removed the
  copying that motivated it, and the VM's register file removes the per-call
  `Vec<Value>`; there may be nothing left.

**Item 11 (lazy iterators) is the exception** — it is orthogonal to the VM, it is the
only thing that addresses `strings` at 18.9× roc's memory, and it is the largest
remaining *algorithmic* win. Do it after V6, or before V0 if the memory number is more
annoying than the speed.

---

## What not to do

Each of these is a plausible idea that either the measurements or the constraints say is
wrong here.

- **`unsafe` anything.** Not for dispatch, not for registers, not for a NaN-boxed
  `Value`. The whole point of writing this in Rust is that a wrong opcode is a panic
  with a message and not a silent memory corruption that shows up as a wrong answer in
  a golden pair three phases later. `#![forbid(unsafe_code)]` is the gate; if a number
  is missed, profile.
- **A JIT, now.** Cranelift is the eventual answer for genuine bare metal, and the VM's
  IR is its input. It is a strictly larger correctness surface on top of a VM that does
  not exist yet.
- **Caching parsing or bytecode between runs.** Covered above: 1-3ms in exchange for a
  staleness-bug class and losing "edit the file, it takes effect".
- **Building the AST cache before something loads a module twice.** A `HashMap` that
  never hits is not a cache, it is a comment.
- **Keeping the tree-walker "just in case" past V6.** Two engines, twice the bugs, and
  gates that no longer say which one is wrong.
- **`criterion`.** The shell harness measures the real binary end to end, which is the
  thing users experience. Per-function microbenchmarks answer a question nobody asked.
- **Parallelism.** Roc is pure, so this is tempting, but `Rc` makes everything `!Send`.
  Revisit only after V6 — the VM's explicit frame stack and slot-resolved globals make
  it a real possibility for the first time, which is another argument for the VM and not
  an argument for doing it now.
- **Arena-allocating the AST.** The AST stops being the runtime representation at V6.
