# Optimization plan

The interpreter is correct — 98 golden pairs, 439 Rust tests, 12 of 19 comparable
language examples byte-identical to `roc`. This is about making it fast **without
giving any of that back**.

Everything below is measured. No item is here because it is a known-good idea in
general; each one has a number attached, taken on this machine from this code.

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

### Results so far

Items 1, 2, 4, 5, 7 and 10 are done, plus `Rc` for lambda parameters. Every gate stayed
green throughout — 98 golden pairs, 439 Rust tests, the same 12 language examples.

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
short script, not evidence that a tree-walker out-executes a compiler. On a long-running
program roc would pull ahead.

Two ceilings are gone rather than merely improved:

- List work is **linear**, not quadratic. `n=4000` was 519ms and `n=8000` 2032ms; they
  are now 4ms and 7ms, and `n=32000` is 21ms.
- A 5-million-iteration loop runs in **2.8 MB**, constant. Building a 40,000-character
  string peaks at 6.3 MB where it used to reach **710 MB**.

`Value` also shrank from 80 bytes to 64, which makes every clone and move cheaper.

### What each change bought

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
it: worth 1-2ms on `matching`, about 7-12%. Detecting recursion needs an AST walk per
lambda and a new field to thread the current function's name. Not worth it — measured,
not assumed.

### Starting numbers

```
benchmark                 time        peak
calls                     80ms      3324kB     naive fib(22) — calls and arithmetic
closure_capture          516ms      6560kB     map+fold, lambda captures the list
list_ops                   4ms      3848kB     IDENTICAL work, top-level lambdas
loop                      27ms     18744kB     200k iterations, no allocation
matching                 224ms      7864kB     180k tag constructions and matches
records                   32ms      6228kB     40k record updates
strings                   30ms     55764kB     8k string concatenations
```

Against `roc` on the same programs: calls 5× slower, loop **2.7× faster**, list work
160× slower. The gap is not spread evenly, which is the useful part.

### The rule for every change

A change lands only when all four are true:

```bash
tests/bench.sh                   # the benchmark it targets improved
tests/check_roc.sh --strict      # 98 pairs still green
tests/check_examples.sh          # 12 examples still match roc
cargo test --quiet               # 439 tests still pass
```

Optimizations break things quietly. The gates are what make "without losing feature
parity" checkable instead of hopeful.

---

## Tier 1 — the two real problems

These are not tuning. They are order-of-magnitude, and everything else is noise until
they are done.

### 1. The environment is deep-copied on every call

`closure_capture` takes **516ms**. `list_ops` does *the same map and fold over the same
4000 elements* and takes **4ms**. The only difference is where the lambda is written:

```roc
# 516ms — the list is in the block the lambda is written in, so the closure captures it
main! = |_args| {
    xs = (1..=4000).iter()
    doubled = xs.map(|x| x * 2)
    ...
}

# 4ms — the lambda is top-level and captures nothing
double = |x| x * 2
main! = |_args| { (1..=4000).iter().map(double) ... }
```

129× for identical work. And it is quadratic — doubling the list quadruples the time:

```
n=1000   46ms      n=4000   519ms
n=2000  133ms      n=8000  2032ms
```

**Cause.** `Environment` holds `scopes: Vec<StackFrame>`, and `#[derive(Clone)]` deep
copies all of it. `apply()` clones the closure's environment per call, so each of the
4000 calls copies a scope containing a 4000-element list. The `top` scope is already
shared through `Rc` (that was the fix for order-independent top-level definitions); the
local scopes are not.

**Fix.** Make a scope a pointer, so cloning an `Environment` is a refcount bump:

```rust
pub struct Environment {
    top: Rc<RefCell<Vec<(&'static str, Value)>>>,
    scopes: Vec<Rc<RefCell<StackFrame>>>,   // the Vec is small; the frames are shared
}
```

The `Vec<Rc<..>>` still allocates per call, but it copies pointers rather than values,
and the frames themselves are shared.

**Watch out:** `assign` mutates a binding in place — that is what makes `var` work
inside a loop. With shared frames a mutation becomes visible to anyone holding the same
`Rc`, which is correct for `var` (a loop body updating an outer `var` is the point) but
must be re-checked against `tests/roc/16_loops/`. `Rc<RefCell<..>>` is the cheap route;
a persistent cons-list environment avoids the `RefCell` but is a bigger change.

**Verify:** `closure_capture` should approach `list_ops`. Anything less than a 10×
improvement means the copy is still happening somewhere.

### 2. Every string is leaked

`strings` peaks at **55 MB** to build a 16,000-character string. Pushed to 40,000
characters it peaks at **710 MB**.

There are 24 `Box::leak` sites. Every concatenation, every interpolation, every
`to_str` leaks its result permanently, so building a string in a loop leaks O(n²) bytes
and never gives any of it back.

**Cause.** `Value::Str(&'static str)`, and `&'static str` can only be obtained by
leaking.

**Fix.** `Value::Str(Rc<str>)`. Clone becomes a refcount bump (it is currently a pointer
copy, so no regression there), and memory is actually freed.

Mechanical but wide — roughly 24 construction sites plus every `match` on
`Value::Str(s)`. Do it in one commit, lean on the compiler, and keep the gates green at
the end. The same argument applies to `Box::leak` in the parser for identifiers, but
those are genuinely program-lifetime and can stay.

**Verify:** `strings` peak RSS should drop by two orders of magnitude. Time should
improve too, since 710 MB of allocation is not free.

### 3. `Value::List(Vec<Value>)` clones its elements

Every list passed to a function, returned from one, or bound to a name copies all of it.
`Value` is 80 bytes, so a 4000-element list is a 320 KB copy.

**Fix.** `Value::List(Rc<Vec<Value>>)`, with `Rc::make_mut` where a builtin genuinely
mutates. Do this **after** Tier 1.1 and re-measure — the environment fix may already
have removed most of the copying, and if so this is not worth the churn.

---

## Tier 2 — free, do them today

### 4. There is no `[profile.release]`

Cargo.toml has no profile section at all, so the release build is the default one.

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
```

Costs nothing but build time. Measure it — on an interpreter dominated by allocation
this may be worth very little, which is itself worth knowing before spending effort
elsewhere.

### 5. The gates run the debug build

`tests/check_roc.sh --strict` takes **9.2 seconds**, and the debug binary is 3-5× slower
than release (`calls`: 623ms vs 211ms).

```toml
[profile.dev]
opt-level = 1        # the interpreter itself
[profile.dev.package."*"]
opt-level = 3        # dependencies, compiled once
```

This does not make the shipped interpreter faster. It makes the loop you work in
faster, which is the thing you actually pay for all day.

---

## Tier 3 — after Tier 1, with the profiler open

Do not guess at this tier. Run `perf record ./target/release/rocflight tests/bench/calls.roc`
first; the ordering below is a prediction, not a measurement.

### 6. Variable lookup is a linear scan comparing strings

`Environment::lookup` walks every scope, and within each scope every binding, comparing
`&str`. In `calls` (80ms of nothing but calls) this runs on every `n`, every `fib`.

**Fix.** Resolve each identifier to a `(depth, slot)` pair at parse time, when the
scopes are already known, and index directly at runtime. This is the standard move and
it is a real change to the AST — `Expr::Ident(&str)` becomes `Expr::Local(u16, u16)`
with a name kept only for errors.

**Cheaper first step:** intern identifiers to `u32` symbols and compare integers. Most
of the win, a fraction of the disruption. `string_pool` already exists in the parser.

### 7. The lambda body is cloned on every call

`src/eval/mod.rs:1702` — tying the recursive knot rebuilds the closure, cloning
`body: Box<Expr>` each time. `Expr` is 72 bytes per node and the clone is deep, so a
recursive function copies its whole body per call.

**Fix.** `Rc<Expr>` for the body. `calls` and `matching` are where this shows up.

### 8. Method dispatch allocates and scans

`methods_named` builds `format!(".{}", method)` on every dispatch, then scans the whole
environment — including the shared top scope, which holds every top-level binding in the
program — doing a `str::ends_with` per entry.

**Fix.** Build the method table once at parse time as a `HashMap<&str, Vec<..>>`. The
parser already collects methods into `self.methods`; hand that to the evaluator instead
of rediscovering it per call.

### 9. Error messages are built eagerly

236 `to_string()` and 138 `format!` sites. Some are on paths taken once; some are inside
`ok_or_else` closures (fine, lazy); a few build a message before checking whether it is
needed. Fix only the ones the profiler names.

---

## Tier 4 — algorithmic

### 10. A `for` over a range materializes the whole range

```rust
Value::Range { start, end, inclusive } => {
    let last = if inclusive { end } else { end - 1 };
    (start..=last).map(Value::Int).collect()      // src/eval/mod.rs:200
}
```

`loop` peaks at **18.7 MB** for a counting loop that allocates nothing of its own —
200,000 `Value::Int`s built up front, 80 bytes each. `for i in 0..<10_000_000` would
need 800 MB before the first iteration.

**Fix.** Iterate the range directly instead of collecting. Small, local, and it removes
a hard ceiling rather than shaving a constant.

### 11. Iterators are eager

`.iter()` materializes (documented as a ceiling in IMPLEMENTATION_PHASES.md). A lazy
iterator would fuse `map` and `fold` into one pass with no intermediate list. Worth it
only once ranges are lazy, since they share the machinery.

---

## What not to do

Listed because each is a plausible idea that the measurements say is wrong here.

- **A bytecode VM or a JIT.** Every problem found so far is representation — copying and
  leaking — not dispatch overhead. A VM would carry all of it along, and cost the
  feature parity that took twenty-two phases to build. Revisit only when the profiler
  says `eval`'s `match` is the top frame, which it currently is not.
- **Caching parsing between runs.** Startup is 1-3ms for real programs. `.rocflight/cache/`
  is deliberately write-only for inspection; making it load-bearing would trade a
  correctness property (edit a file, it takes effect) for nothing measurable.
- **`criterion`.** The shell harness measures the real binary end to end, which is the
  thing users experience. A new dev-dependency to measure functions in isolation would
  answer a question nobody asked.
- **Parallelism.** Roc is pure, so this is tempting. But the environment is `Rc`-shared
  and `!Send`, so it would mean a different design. There is 129× available from single
  threaded work first.
- **Arena-allocating the AST.** Plausible later, but `Rc<Expr>` (item 7) gets most of it
  for a fraction of the disruption.

---

## What is left

Items 3, 6, 8, 9 and 11 remain. None of them has a measurement behind it yet, and this
machine has no `perf` or `valgrind` — so the honest way to take the next one is the way
these were taken: make the change, run `tests/bench.sh`, keep it or revert it.

The remaining time is now spread fairly evenly across per-call work — `matching` does
180,000 calls in 55ms, about 300ns each, and `calls` runs at a similar rate. There is no
single dominant cost left to remove, which is the sign that the cheap wins are spent.

- **Item 6 (slot-indexed variables)** is the standard next step and the one most likely
  to pay, since `lookup` still compares strings on every identifier. Start with the
  cheap half: intern identifiers to `u32` and compare integers.
- **Item 3 (`Rc<Vec<Value>>` lists)** may already be unnecessary — item 1 removed the
  copying that motivated it. Measure before writing any of it.
- **Items 8, 9, 11** are small and speculative. Do them only if a profiler names them.
