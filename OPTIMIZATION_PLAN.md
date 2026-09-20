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

The median eval test costs 5.5ms; a trivial one costs 0.85ms. **The first version of
this plan read the 4.6ms in between as front-end work on a constant, and that was
wrong** — Phase 1 measured it. Two things the table does not show:

- Only about 300 lines across the ~2,100 test sources mention `Dict`, `Set`, `Try`,
  `Box` or `Stream`, so the overwhelming majority of eval tests **load no builtin
  member at all**. The 3.5ms breakdown above is the `Dict` case, and the `Dict` case is
  rare here.
- The harness gives each rocflight test its own scratch directory, writes `main.roc`
  into it, forks, execs, reads the answer and deletes the tree — 12 of those in
  parallel. A trivial program's own latency is 0.85ms alone and 1.33ms under the same
  12-way load; the rest of the median 5.5ms is the harness and the operating system,
  which roc's in-process backends never pay.

So this table is **not** the measure of rocflight's front end, and Phase 1 moved it by
5% rather than halving it (13.7s → 13.0s, mean 6.8 → 6.4ms, median 5.5 → 5.1ms). What
Phase 1 did move is the programs the breakdown above is actually about: a `Dict` program
is 27% faster end to end and a `.map` program 38%. Take the suite as the correctness
gate it is, and read speed off single programs.

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

A second probe earned its place while Phase 4.1 was being diagnosed:
`ROCFLIGHT_CODE=1` dumps every compiled chunk's opcodes and constants — the bytecode
counterpart of the existing `--show-ast`. It is what showed that 4.1's stated cause was
not the cause, so it stayed.

`crate::timing()` and `crate::tick()` in `src/lib.rs` are the whole of the timing side: `timing`
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

## Phase 1 — stop re-deriving constants at run time — **1.1, 1.2, 1.3a done**

`Builtin.roc` is `include_str!`'d. Its member boundaries and its low-level dependency
graph are **functions of a constant**, and `build.rs` already existed. Both were being
recomputed in every process; they are computed once, at build time, now.

What it bought, wall clock, median of 150 runs:

| program | before | after |
|---|---|---|
| `1 + 2` | 852µs | 849µs — nothing to win, and nothing lost |
| `"hello".len()` | — | 1079µs |
| 3-element `.map` | 2366µs | **1460µs** (−38%) |
| `Dict` insert/get | 5329µs | **3876µs** (−27%) |

And in-process, on the `Dict` program: `slice + index` 0.93 → 0.17ms, `reachable`
0.80 → 0.13ms, `signatures_for(Dict)` 0.40 → 0.36ms, `builtin::load` 3.41 → 2.03ms.

### 1.1 — the member index moves to build time — **done, −0.76ms**

`index()` was a `OnceLock` scanning all 700kB of `SOURCE` for each member's byte range.
Once per process was still 0.93ms of every program that names a `Dict`.
`build.rs::scan` does it now and writes `MEMBERS` out as a static table that
`builtin.rs` includes; `member_name` went with it, and with it the `Box::leak` per
member name — the names are string literals in the generated file.

`tests/check_builtin.sh --strict` is the gate rather than a table-equality test: a
skewed offset makes a member fail to parse, and that script reports parse results member
by member.

### 1.2 — the reachability closure moves to build time — **done, −0.67ms**

`reachable()` cut the low-level section down to what the other loaded members mention,
by closing over the text word by word and allocating a `String` per word, over 2,305
lines, in every process. `build.rs::low_level_reach` answers it per member now
(`LOW_LEVEL_REACH`); the run-time half unions the lists of whatever is loaded and splits
the blocks in one pass.

The union is exact because reaching is monotone: the closure of two members' words is
the union of each member's closure. That is the same property the old code relied on
when `Dict` and `Set` arrived together.

Evidence it kept the same declarations, which is what a faster wrong answer would have
broken: the pruned section still parses to 454 lines at the same 0.66ms, and
`check_builtin.sh` still reports 164 definitions and 153 intrinsics for it.

### 1.3 — `signatures_for` — **half done, −0.07ms; the other half is not sound**

`signatures_for(module)` is what the checker asks for a builtin's declared type, and it
parsed on first use. Broken down with the Phase 0 probe (median of 25), for `List`:

```
members_where + annotations_only ->  137 lines   0.118ms
desugar                                          0.006ms
parse                                            0.368ms   <- 2.7µs a line
filter + normalise                               0.042ms
signatures_for(List)                             0.572ms
```

**1.3a, done.** `annotations_only` now reads the member's lines straight off `SOURCE`
(`member_lines`) instead of having `members_where` build all 1,676 as a `String` so that
1,539 could be thrown away, and the `Module.` prefix the signature filter compares is
formatted once instead of once per signature — `Num` declares 828 of them. `List`
0.639 → 0.572ms, `Dict` 0.403 → 0.355ms.

That is a tenth of what the first draft of this plan projected, because the 0.25ms it
attributed to `members_where` was a single-shot reading of a cold page cache. **Medians,
not single runs, for anything under a millisecond.**

**1.3b, not done, and not as written.** The plan said to generate the signature tables
in `build.rs`. That cannot work: a signature is a `types::Type` produced by *this
crate's* type parser, and a build script cannot call the crate it is building. The
options left are a checked-in generated file with a regeneration step, or reusing what
`builtin::load` already parsed — and the second is **provably wrong for `Set`**:
`Set(item) :: Dict(item, {})`, so `Set`'s signatures only carry their element type if
`Dict`'s declaration was in scope while they were parsed, which is exactly why
`parse_signatures` prepends `Dict`'s annotations. `load` parses each member with its own
`Parser`, so its `Set` signatures are the degraded ones. It would be sound for `Dict`
alone, for 0.36ms on `Dict` programs, at the price of a side channel between `load` and
`signatures_for`. Left for whoever needs that 0.36ms.

The real cost here is the **0.37ms to parse 137 annotation lines**, at 2.7µs a line
against ~1.4µs for ordinary source. That is Phase 3's number, not Phase 1's.

### 1.4 — `needed_by` — **measured, not worth it**

`needed_by` is four substring tests: `source.contains("Dict")` loads `Dict`, `Set` and
the low-level section, 2.0ms, even for a program that merely mentions the word.

Narrowing it precisely needs the parsed AST — which qualified names the program actually
references — and that is real machinery. The cheap version, whole-word matching on
non-comment lines, was measured against every `.roc` in the repo: it changes the answer
for **one** file, `tests/roc/14_nominal/field_optional.roc`, where `Try` appears only in
a comment, worth 0.35ms once. And across roc's ~2,100 eval test sources only about 300
lines mention any trigger word at all, so there is no aggregate there either.

Left undone deliberately. Revisit if a real program shows up paying 2ms for a word in a
comment.

### Measured and rejected

- **Stripping comments before the reachability closure.** A name that appears only in a
  doc comment cannot be called by anything, so following it keeps a declaration no code
  reaches — and `Builtin.roc`'s doc comments are full of `## expect Dict.insert(…)`.
  Since 1.2 runs at build time this was free to try, and it changes nothing: `Dict`
  reaches 48 low-level declarations and `Set` 41, with or without the filter. Every name
  mentioned in a comment is also used in code. Reverted.

### What Phase 1 leaves

On the `Dict` program, of ~2.9ms in-process:

```
the three member parses        1.70ms   <- 59%, and Phase 2 or 3 owns it
signatures_for(Dict)           0.36ms   <- 1.3b, if the side channel is worth it
compile to bytecode            0.35ms
slice + reachable              0.30ms
run                            0.07ms
```

The front end is still the program, but what is left of it is **parsing Roc source**,
not re-deriving tables. That is the hand-off to Phase 2 and Phase 3.

---

## Phase 2 — parse `Builtin.roc` once, not once per process

Phase 1 leaves 1.70ms of parsing three members and 0.35ms of compiling them on a `Dict`
program — 59% and 12% of what is left. Same observation as Phase 1, the input is a
constant, but the output is an AST with node identity, interned `&'static str` and source
offsets, so this one is real work rather than a table.

**And it cannot be done in `build.rs`.** Phase 1.3b found the reason the hard way: a
build script cannot call the crate it is building, so nothing that needs rocflight's own
parser or `types::Type` can run there. Every option below therefore needs a *checked-in*
artifact produced by a tool that links the crate — `cargo run --bin <gen>` — plus a test
that regenerates it and compares, so a re-sync of `Builtin.roc` cannot leave the artifact
stale. That shape is the real cost of this phase, not the loading code.

- **2a — a generated Rust file that builds the AST.** Fastest possible load: no parsing,
  no deserializing. Inflates `cargo build`, and the file is large. Try it on `Set` (230
  lines) alone and measure both the load win and the build cost before going further.
- **2b — a serialized AST, deserialized per member on demand.** Smaller build impact and
  lazy per member. Costs a serializer, a deserializer and a format that has to stay in
  step with `ast::Expr` — the most code of the three.
- **2c — cache the compiled `Program` under `.rocflight/`, keyed by the hash of
  `Builtin.roc` and the member set.** Least code, needs no generation step, but every
  process still pays a deserialize and correctness rests on a cache-invalidation rule.

Awkward either way: `NodeId` carries a source offset for error locations, and a
pre-built AST has no offset into the *user's* file. That is fine — a runtime error inside
`Dict.insert` should point at the vendored source, which is what it does today — but it
has to be deliberate, because `ast::locate` silently returns `None` and the error just
loses its location.

**Prefer Phase 3 first.** Phase 3 attacks the same 1.70ms through the parser, helps every
module a user imports and every annotation they write, and needs no generated artifact or
regeneration discipline. Come back here only if the parser work stalls.

---

## Phase 3 — the parser — **the cheap items done; the lexer is a decision, not a task**

~1.4µs per line of ordinary source. No sampler on this machine can break that down, so
this phase was a sequence of guarded experiments. Three landed, one was refused on
arithmetic, and the biggest win was not on the list at all.

Parse time is **linear** in input size — 100/200/400/800/1600 lines of the same shape
give 0.24/0.46/0.88/1.71/3.43ms — so there was no quadratic in the general path. There
was one in a specific path, and finding it was the whole of this phase's value.

### Where parse time goes, per construct

Measured at 800 lines of each shape, which is how the items below were chosen:

| 800 lines of… | before | after |
|---|---|---|
| bare bindings, `f = 1` | 0.690ms | 0.646ms |
| annotated bindings, `f : U64` then `f = 1` | 1.451ms | **1.253ms** |
| string literals, no nominals in scope | 0.819ms | 0.723ms |
| string literals, **20 nominals in scope** | 5.585ms | **0.784ms** |

A minimal top-level binding costs ~0.65µs on its own; a binop adds ~0.25µs and a lambda
~0.66µs. That is where the remaining time is — diffuse, not in a hot spot.

### Done: the nominal table was deep-copied per string literal — −86%

`parse_string` cloned `self.nominals` and `self.nominal_defaults` for **every string
literal in the file**, for no reason but the borrow checker: `nominal_literals` is
borrowed mutably alongside them, and `&mut self` cannot hand out both at once. A `Type`
is a tree, so each copy walked every declaration — quadratic in (literals × nominals).
Naming the fields (`let Parser { nominals, nominal_literals, .. } = self`) splits the
borrow and the clones go away.

This is the kind of thing the per-construct table above exists to find: nothing in the
profile of a *typical* program pointed at it, because the cost only appears when a file
has both many nominals and many strings.

### Done: the string pool — −4% to −6% everywhere

`string_pool::intern` took a global `Mutex` and hashed with SipHash, once per identifier,
string literal and field name in every file. It is now a thread-local pool with a
ten-line FNV-1a hasher — the keys are identifiers of a few bytes, where SipHash's setup
costs more than its hashing, and the standard library ships nothing faster.

Single-threaded is safe by construction here: `vm::RUNNING` and the AST's node table are
thread-locals already, so the only other threads are the test harness's and each parses
its own source. Two threads interning the same text would get two pointers, which costs a
leak and never an answer — every comparison of a name in the interpreter is by content,
and nothing outside the pool's own tests looks at an address. The `StringPool` struct was
dead outside its module and went with the rewrite, as did `leak_field`'s body, which
leaked a fresh copy of every field name rather than interning it.

### Done: a 400-character `String` per annotation line

`capture_type_annotation` collected a 400-char window into a `String` to look for a
`where` clause, on every annotation line, and `Builtin.roc` is mostly annotation lines.
It is a borrowed slice now.

### Refused: `Parser::input: String` → `&'a str`

This was the plan's first item, on the theory that `Parser::new` copies every source it
is handed. It does — and the copy is one ~20kB memcpy, about 0.7µs, against 646µs of
parsing the same file. 0.1%, in exchange for a lifetime parameter through 134 call sites.
Priced, not attempted.

### What it bought where it matters

| | before | after |
|---|---|---|
| `Dict` member parse | 0.667ms | 0.638ms |
| `(low level)` member parse | 0.641ms | 0.601ms |
| `builtin::load`, whole | 1.971ms | 1.901ms |
| `signatures_for(List)` | 0.525ms | 0.497ms |
| `Json` example, end to end | 2062µs | 1959µs |
| `GraphTraversal` example | 8492µs | 8319µs |
| `Parser` example | 7188µs | 7048µs |

### Still open: the lexer, which is a decision and not a task

What is left is ~200ns per token spread evenly across the per-statement and
per-construct paths. There is no remaining hot spot to shave — the table above is what
says so — and the way to cut it is to tokenize once instead of probing the source at 269
`self.input[self.pos..].starts_with(…)` sites.

That is a rewrite across 5,903 lines with 13 places that save and restore `pos`, and the
behaviour may not move by a millimetre: 1,953 eval tests and 99 golden pairs compare
exact output, and the desugarer feeds this parser. It is a multi-session commitment with
a real chance of a subtle divergence, for a prize of perhaps 40% of parse time — which is
~0.7ms on a `Dict` program and ~1µs on a small one.

**Do not start it as a side quest.** If it is worth doing, it is worth doing as its own
piece of work, behind the differential suite, with the golden pairs run on every commit.

### Not attempted: AST node allocation

`Expr` boxes its children, so a member's parse is thousands of small allocations, and
`fresh_node` costs a thread-local access and a `RefCell` borrow apiece. Bounding it by
stubbing `fresh_node` out failed — node identity is load-bearing for annotations and
nominal literals, so a program with every id equal to zero does not parse — and the
indirect evidence is weak: adding a single `env::var_os` check inside `fresh_node` cost
44%, which says only that it is called about three times per line. An arena would touch
every file in the crate. Measure it properly before anyone tries.

---

## Phase 4 — VM per-op cost

~15ns per opcode. The inline `fold` loop is four opcodes per element and runs at 46ns,
which is consistent, so this is a per-op number and not a callback ceiling: callback
pooling was measured and removed, `fold`/`map` are already lowered into in-frame loops,
and a literal lambda is already inlined into them.

Ordered by measured evidence, strongest first.

### 4.1 — the 3× was never `BinInt` — **done, −31%**

The claim was that `t = t + 1` misses the specialised integer opcode, because that loop
ran at 508–572ms where the same loop with a variable operand ran at 104–174ms, same
answer. **It does not miss it.** Both loops emit `BinInt { op: Add, width: 192 }`. What
differs is the constant pool:

```
t + x:  consts [Int(0), Int(1),   Int(3),   …]
t + 1:  consts [Int(0), Dec(1.0), Dec(3.0), …]
```

The loop variable in the second version is `_x`, unused, so nothing constrains the
range's element type and its numerals default to fractional — roc's own rule, and the
right answer. But a `Dec` range is not a `Value::Range`: `MakeRange` gives a
`Value::Iter(Lazy::Range)`, and that is walked a lazy step at a time. The 3× was the
iterator, not the arithmetic.

`Lazy::Range::step` was paying, per element:

- **six `apply_binop` dispatches**, two of which recomputed `descending` — a property of
  the step value that cannot change while the range is walked — by subtracting the step
  from itself to get a zero of its own kind and then comparing against it;
- **a fresh `Rc<Lazy>`** of about 168 bytes for the rest, because `step` was pure by
  construction: "never mutating in place, exactly as roc's does".

Both are gone. `negative(step)` reads the sign directly (−17%), and `step` takes its
`Rc` **by value** and advances a list or a range in place through `Rc::make_mut` (−20%
more), so walking n elements allocates once instead of n times. `make_mut` is
copy-on-write, so a caller that kept its own handle still gets a fresh rest and observes
nothing — the purity that mattered is preserved, only the allocation is not. The two
variants' old arms were **deleted** rather than left as a second implementation:
`step_wrapped` holds the wrapping iterators, which still rebuild themselves, and `step`
hands anything else straight to it.

| | before | after |
|---|---|---|
| `Dec` range, 2M elements | 347ms | **241ms** |
| `F64` range, 2M elements | 290ms | **192ms** |
| `iter_range` benchmark | 316ms | **222ms**, and peak RSS 5.1 → 4.3 MB |
| integer range, 2M elements | 103ms | 104ms — untouched, as intended |

Where the rest of it goes, measured rather than assumed: an `F64` range, whose
arithmetic is one of `apply_binop`'s fast paths, is within 25% of the `Dec` one, while an
integer range is 2.3× faster than either. So the residual is the lazy machinery — three
operator dispatches and a `Value` clone per element — and 4.2 is worth only about 25ns of
it here.

### 4.1b — the wrapping iterators too — **done, −20% on a chain**

`map` over `filter` over a list rebuilt three `Rc`s per element, because each wrapper
stepped a CLONE of its inner and then built itself around the rest. It now allocates
none. `step` is a shim over `advance(&mut self, out: &mut Value)`, which mutates the
iterator in place and recurses into a wrapper's own `inner` through `Rc::make_mut`, so a
chain advances down its layers under one mutable borrow instead of rebuilding itself on
the way back up. Cloning the inner also defeated the leaf fix from 4.1 — a list under a
`map` was stepped at a refcount of two, so it copied itself every element.

| | before | after |
|---|---|---|
| 2-layer chain (`keep_if` then `map`), 400k | 124ms | **101ms** |
| 4-layer chain, 400k | 155ms | **122ms** |
| `iter_range` — no wrapper | 217ms | 217ms |
| `Dec` range — no wrapper | 238ms | 241ms |
| `F64` range — no wrapper | 190ms | 194ms |

**Two things had to be right, and the obvious version got both wrong.** The first draft
put every variant in one recursive `advance`; chains gained 14% and a plain range **lost
10%**, which would have been a regression on the commoner path to speed up the rarer one.

- **The leaves cannot share a recursive function with the wrappers.** With `List` and
  `Range` in the same `advance` as the six wrappers, LLVM keeps the whole thing out of
  line. `#[inline]` did not fix it. `advance` holds the two leaves and delegates;
  `advance_wrapped` holds the wrappers and recurses back into `advance`. Necessary, and
  still 7% short.
- **The item goes through an out parameter.** `Result<Made, EvalError>` with a `Value`
  inside moves 80 bytes per layer per element — a 48-byte `Value`, a `String`-carrying
  error, and 16-byte alignment. With `Made` reduced to a three-case tag and the item
  written through `&mut Value`, the leaf paths came back to parity and the chains kept
  their 20%.

Neither would have been visible against `tests/bench/baseline.tsv`, which reads ±10% on
an untouched tree. A/B against a copied pre-change binary is what caught them.

What is left in this area is small: `Lazy::Concat` clones the second iterator when the
first runs out, once per concatenation, and `fold_native` and `materialize` hold their
own handle so the first step of a walk still copies. Neither is per-element.

### 4.2 — `Dec` arithmetic

A 2M-element fold with a `Dec` accumulator is 310ms against 92ms for `I64`. Note that
4.1 showed most of a `Dec` loop's cost was the iterator rather than the arithmetic —
against an `F64` range the `Dec` one is only 25ns an element slower, over three
operations — so the number to beat here is closer to 8ns an operation than 110ns. `apply_binop` reaches `Dec` only after an `Int` probe, a
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

## Phase 5 — lower the remaining callback builtins — **eight done, one refused, the rest blocked**

`fold` and `map` compiled into in-frame loops; the rest re-entered the VM per element
through `call_closure` — a fresh register file, an argument `Vec` and a **Rust** frame
apiece. Four more are compiled now.

| over 2M elements | before | after |
|---|---|---|
| `all` | 221ms | **118ms** |
| `find_first` | 230ms | **106ms** |
| `fold_try` | 328ms | **205ms** |
| `fold_with_index` | 273ms | **161ms** |
| `find_first_index` | 262ms | **146ms** |
| `find_last_index` | 258ms | **166ms** |

`Compiler::list_loop` took a `fold: bool`; it now takes a `Shape`, and what each element
does with the callback's answer is `finish_element`. The new opcode is `TestBool`, which
jumps unless a register holds exactly `Bool(want)` — deliberately **not** `JumpFalse`,
which errors on anything that is not a `Bool`. The builtins ask
`matches!(value, Bool(b) if b == want)`, so a non-`Bool` predicate is simply not a match
there, never a message, and compiling to `JumpFalse` would have invented an error the
interpreter does not have.

### What is safe to lower, and why

`any`, `all`, `count_if` and `find_first` are declared on `List` alone in `Builtin.roc`
and each answers a plain **value** — a `Bool`, a `U64`, a `Try`. That is the property
that matters: the answer does not depend on whether the receiver was a `List` or an
`Iter`, so the loop can stand in for the builtin whatever the checker believes about the
receiver.

`count_if` was not implemented at all — `List.count_if` was an unknown function, though
`Builtin.roc` declares it — so the Rust builtin was written too. A method should not
exist only in the compiler: the lowering is an optimization, not the only way to reach it.

`fold_try`, `fold_with_index`, `find_first_index` and `find_last_index` pass the same
test, and `Shape` now answers four questions about itself — `takes_init`, `arity`,
`needs_index`, `passes_index` — so the loop body is one path instead of three.

Two things in there needed care:

- **`fold_try` is a three-way branch.** An `Err` is the answer and stops the fold; an `Ok`
  unwraps into the accumulator; anything else becomes the accumulator as it stands, which
  is what the builtin's `other => acc = other` does. `TestTag "Ok"` already matches a
  non-tag value and `GetPayload` answers one with itself, so both fall out of one test.
  Testing for `Ok` explicitly rather than trusting the `Err` test to be exhaustive is what
  keeps a payload-less tag from indexing an empty payload and panicking. Reaching the end
  without an `Err` wraps the accumulator in `Ok` **after** the loop, which the early exit
  jumps past.
- **"The loop needs a position" is not "the callback gets the position".** Only
  `fold_with_index` is handed it; `find_first_index` reports a position but its predicate
  takes just the element. Conflating them passed the index as a second argument and made
  `xs.find_first_index(big)` fail with "Lambda expects 1 argument(s), got 2".
  `needs_index` drives register allocation and `passes_index` drives the argument list.

`IterNext`'s own `idx` cannot serve as the position, either: it is the element index only
for a list or a range, and a lazy iterator carries its own state and never touches it. The
loop keeps its own counter and advances it right after `IterNext`.

### Refused: `keep_if` / `drop_if`

Tried, measured at 414ms → 153ms, and reverted. It also *fixed* four cases where
rocflight disagreed with roc — `[1, 2, 3].keep_if(p)` answered a lazy `Iter` and
inspected as `<opaque>` where roc gives `[2, 3]` — which made it tempting. But
`Builtin.roc` declares **both** `List.keep_if -> List(a)` and `Iter.keep_if -> Iter(a)`,
and the second is lazy, so a compiled loop may only stand in for the List one. Checked
against the real `roc` binary:

```
Str.inspect((1..=5).iter().keep_if(|x| x > 3))
  roc  <opaque>      before  <opaque>      lowered  [4.0, 5.0]
```

There is no sound guard for it. `dispatch_modules` is not one — the checker calls
`(1..=5).iter()` a `List`, which is what produced that line. Nothing syntactic is one
either, because `xs = (1..=n).iter()` and then `xs.keep_if(p)` has a bare name as its
receiver. Trading a correct `Iter` case for a correct `List` case is not progress, so
neither was taken: `dispatch_builtin` still answers the lazy one for both, and says so.

### The divergence underneath, which is not an optimization problem

`.iter()` on a list **is** the list at run time, so nothing after the checker can tell
`xs.keep_if(p)` from `xs.iter().keep_if(p)`. That is also why
`Str.inspect([1, 2, 3].iter().map(f))` answers `[2.0, 4.0, 6.0]` where roc answers
`<opaque>` — a pre-existing divergence this phase did not introduce and did not fix.
Both need an `Iter` that is its own value rather than a borrowed name for a list, which
is a representation change and belongs in a correctness plan, not this one.

### Still open, and what blocks it

Every value-returning callback method on `List` is compiled now. What is left answers a
**container**, and each is blocked on the same `Iter` question as `keep_if`:

| method | why it is still a builtin |
|---|---|
| `keep_oks` | answers a `List`; needs the `Iter` question settled |
| `update_at` | not a walk over every element — it takes an index |
| `sort_by` | a sort, not a loop; the callback is a key function |
| `sort_with` | a sort with a comparator; cannot be a loop at all |

`keep_oks` is the only one of the four that a loop would even fit, and it is worth noting
that it is *probably* safe — `Builtin.roc` declares it on `List` alone, so unlike
`keep_if` there is no lazy twin to be confused with. It was left for whoever settles the
`Iter` representation, because the same commit should do both.

Lowering these also removes the last place where "calls do not recurse in Rust" stops
holding — `call_closure` nests a Rust frame, so `map` inside `map` inside `map` is still
bounded by the Rust stack with no Roc-level diagnostic.

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

1. ~~**Phase 0**~~ — done. Without it every later phase is an opinion, and it is what
   corrected three of this list's own claims.
2. ~~**Phase 1**~~ — 1.1, 1.2 and 1.3a done: a `Dict` program 27% faster end to end, a
   `.map` program 38%. 1.3b and 1.4 measured and left undone, with reasons above. It was
   billed as halving the eval suite and moved it 5%; see
   [What that costs the suite](#what-that-costs-the-suite) for why that projection was
   wrong.
3. ~~**Phase 4.1**~~ — done, and it was not what it said: the 3× was a `Dec` range being
   walked as a lazy iterator, six operator dispatches and an allocation per element, not
   a missing `BinInt`. `Dec` and `F64` ranges are ~31% faster and `iter_range` went
   316ms → 222ms. **Confirming the mechanism first is what stopped a fix for a bug that
   does not exist.**
4. ~~**Phase 4.1b**~~ — done: the wrapping iterators advance in place too, so a chain is
   ~20% faster and allocates nothing per element. Getting there needed two corrections
   that only an A/B against the pre-change binary exposed; see 4.1b.
5. ~~**Phase 3**~~ — the cheap items done: a quadratic that deep-copied the nominal
   table per string literal (−86% where it bites), the string pool (−4% to −6%
   everywhere), a 400-char copy per annotation line. The plan's own first item was
   refused on arithmetic. What is left is the lexer, which is its own piece of work —
   see the phase.
6. ~~**Phase 5**~~ — eight methods compiled into in-frame loops: `fold`, `map`, `any`,
   `all`, `count_if`, `find_first`, `find_first_index`, `find_last_index`,
   `fold_with_index`, `fold_try` (−36% to −54% each), and `count_if` implemented at all
   for the first time. `keep_if`/`drop_if` refused — it would have traded one roc
   divergence for another. Every value-returning callback method on `List` is now
   compiled; the four that remain answer containers and are blocked on the `Iter`
   question.
7. **Phase 4.2**, `Dec` arithmetic — smaller than it looked before 4.1 measured it.
8. **Phase 4.3–4.5**, the general per-op work, each item measured on its own.
9. **Phase 2**, only if Phase 3 stalls: it needs a checked-in generated artifact and the
   discipline to keep it fresh, which Phase 3 does not.
10. **Phase 6**, or never. Now measured: rocflight's own startup is ~350µs over
   `/bin/true`, which is 0.7s of the suite's 13.0s.

Every phase, the same gates, and the eval suite at 1953 of 1953. A phase that cannot
hold that number does not land, however good its benchmark looks.

Two method notes, both earned the hard way in this document:

- **Anything under a millisecond is a median of 15–25 runs, never a single one.** Two of
  Phase 1's projections came from single readings of a cold page cache and were out by
  3–4×.
- **Confirm the mechanism before writing the fix.** Three entries here — Phase 1.3b,
  Phase 2's build-script plan and Phase 4.1's whole diagnosis — were wrong about *why*
  while being right that something was slow. Each was caught by dumping what the code
  actually does, and each would otherwise have been a day spent on the wrong thing.
