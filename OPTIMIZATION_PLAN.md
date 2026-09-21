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

**1.3b, DONE — and it is what the front end costs now.** See below for why it stopped
being optional. What follows is the reasoning that left it undone, which still holds for
the `Set` half.

**Why it was left.** The plan said to generate the signature tables
in `build.rs`. That cannot work: a signature is a `types::Type` produced by *this
crate's* type parser, and a build script cannot call the crate it is building. The
options left are a checked-in generated file with a regeneration step, or reusing what
`builtin::load` already parsed — and the second is **provably wrong for `Set`**:
`Set(item) :: Dict(item, {})`, so `Set`'s signatures only carry their element type if
`Dict`'s declaration was in scope while they were parsed, which is exactly why
`parse_signatures` prepends `Dict`'s annotations. `load` parses each member with its own
`Parser`, so its `Set` signatures are the degraded ones. It is sound for everything
else, though, and Phase 6 is what made 0.36ms worth having: with process startup gone,
the front end is **~95% of a `Dict` program's wall**.

So `builtin::seed_signatures` hands the cache what `load` already parsed, for every
member but `Set`. `load` sees strictly more than the re-parse — the whole member, with
its own declarations in scope, against annotation lines alone. Seeding only, never
overwriting.

```
Dict program, in process:  3.22ms -> 2.84ms   (-12.7% of its wall)
  signatures_for(Dict)     0.353ms -> gone
  type check               0.413ms -> 0.061ms
```

And it is not only faster: `tests/check_examples.sh` goes from **19 passing to 20**, one
fewer PENDING, because a full parse infers what an annotations-only one could not. The
README has claimed 20 for a while; it is 20 now.

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

## Phase 2 — `Builtin.roc` parsed AND compiled at build time — **done**

`src/artifact.rs` writes the parsed trees out at build time and reads them back.

```
builtin::load   1.821ms -> 0.339ms        a Dict program   -39.7%
```

A four-line `Dict` program is **1.14ms of rocflight** now, against 3.5ms when this
document was rewritten and 2.59ms before this phase.

### What made it fast, which is not "it is a binary format"

**Every name is borrowed from the blob, not interned.** The strings live in the binary's
read-only data, so reading one is a slice — no allocation, no hashing, nothing on the
heap. That is sound because every comparison of a name in the interpreter is by content,
which `memory::string_pool` already documents, and it is the single biggest reason this
beats parsing rather than merely differing from it. Phase 3 is what made it uniform:
`ast::Expr` and `types::Type` both name everything with a `&'static str`.

**Opening the artifact reads no trees.** The first cut decoded all eight bodies just to
find where each began — 0.737ms for the first member, which was most of what the phase
saves. Each body carries a fixed-width length now and the header steps over it; the same
member costs 0.103ms.

### The three things that were not obvious

- **Node identity is a rebase, not a reproduction.** `NodeTable.offsets` is a `Vec<u32>`
  and an id is its index, so a member stores its offsets and its ids relative to its own
  first node; `ast::push_nodes` installs the offsets and answers the base to add. A test
  checks the offsets come back identical, not just the tree.
- **The low-level section is stored four times.** Every other member parses the same
  however it was reached. That one is cut down to what the OTHER selected members use,
  so `Dict` alone and `Dict` beside `Stream` are different trees. `needed_by` can reach
  it exactly four ways, so it is keyed by which.
- **`StrPart::Expr` is a leaked `&'static Expr`**, so decoding one leaks as the parser
  does. Anything else would have changed a lifetime the AST depends on.

### Staleness is a build error, never a wrong answer

The artifact records an FNV-1a of the source it was made from. `build.rs` hashes
`src/roc/Builtin.roc` and **refuses to compile** if they differ, naming the command to
regenerate. Checking at run time would mean hashing 700kB on every startup, which is
most of what this saves. `tests/check_artifact.sh` is the other half — it regenerates
and diffs, which catches a change to the AST or the parser that leaves `Builtin.roc`
untouched. A MISSING artifact is not an error: `build.rs` leaves an empty one and `load`
falls back to parsing, correct and slow.

Two tests hold the format together: a member round-trips to the same tree (compared with
the node numbers blanked, since the rebase is exactly what should differ), and its node
offsets survive.

### Phase 2b — and then COMPILED, not just parsed — **done, −56% more**

Parsing was the phase; compiling was not, and once it landed `compile` was the largest
number left: 0.018ms for a trivial program, 0.604ms with a `Dict`. Every run turned
`Builtin.roc`'s 1,443 definitions into bytecode again.

**The precondition was measured before anything was written.** Four programs were
compiled and their bytecode diffed — including one defining nominal operator methods,
which flips a program-wide compiler switch. With chunk NUMBERS normalised, every named
builtin chunk is byte-identical across all four. The builtin bytecode does not depend on
the program that loads it. Only the numbering did: nested lambdas inside builtin members
were numbered after every top-level binding, and a user program contributes some.

So the numbering changed. `compile_unit` runs in **two phases**: the precompiled group
gets its own top-level chunk (id 0) and its own block of function ids, and everything
else is numbered after it. That block is then the same for every program. The group's
top level becomes the program's `prelude`, run first so the builtins' globals are bound
before anything reads one. **That landed on its own and read 1953 of 1953 before a
single prefix existed** — it is a layout change, not a behaviour one, and proving it
separately is what made the rest safe to build on.

```
builtin::load   0.339ms -> 0.235ms      compile   0.604ms -> 0.088ms
a Dict program           -56% of its wall
```

`run.rs` skips the builtin modules entirely when a prefix is loaded, and nothing decodes
the builtin trees either — `member_tables` reads the declared types the checker wants
and stops before the tree, which is why `load` fell too.

The `Op` codec is generated from ONE table by a macro. An opcode codec is exactly where
a writer and a reader drift apart, and a drift there decodes as a DIFFERENT PROGRAM
rather than as an error; with both directions from one list, adding an `Op` variant
without listing it is a non-exhaustive-match error.

#### Two bugs, both found by gates rather than by reading

- **The artifact was not reproducible.** Same size, different bytes, twice running:
  `methods`, `methods_by_name` and `nominal_shapes` are `HashMap`s and went out in
  iteration order. And generating an artifact READ the previous one, so the trees came
  back with their nodes already installed and the ids differed. `check_artifact.sh`
  caught both, which is the whole reason it regenerates rather than trusting.
- **Some spans point outside the builtin node range.** A group's top level begins with
  the compiler pointing at the APP's node, which means nothing in a prefix another
  program reuses. The offset wrapped — and in release that is silently a nonsense error
  location, not a crash. `tests/check_roc.sh` **on the debug build** is what caught it,
  which is the argument for that gate running debug.

#### What it costs

The binary is 13.3MB → 15.6MB: 228kB of artifact and the rest codec. Six compiled
prefixes are stored because `needed_by` can reach the low-level section six ways, and
four of those contain `Dict` and `Set`, so that bytecode is stored four times. Worth
revisiting only if binary size starts to matter.

### Phase 2c — and stop decoding what nothing reads — **done**

With the bytecode precompiled, what a run still did was decode `Builtin.roc`'s declared
TYPES for the checker — and most of that was decoded into nothing. **Only `Dict` is ever
asked for its signatures**: `signatures_for` serves four modules, two of which are never
loaded, and `Set`'s must be re-parsed beside `Dict`'s anyway. The low-level section's 153
signatures and `Set`'s 32 were built and dropped, every run.

A member's sections are ordered by who wants what now, each expensive run behind a length
so it can be stepped over rather than decoded:

```
intrinsics | signatures | nominals | node offsets | tree
```

`member_tables` reads the intrinsics and the nominals — what every run needs — and steps
over the rest. `signatures_of` reads one member's when `signatures_for` asks, at most
once and only for `Dict`.

```
builtin::load   0.235ms -> 0.018ms        a Dict program  -9.6%
```

The bytecode section is FIXED-WIDTH rather than varint, since binary size is not the
constraint here. It bought 0.146ms → 0.136ms, so the honest reading is that the cost is
materialising 99 chunks' `Vec`s and not decoding them. Without `unsafe` that is the
floor, and `#![forbid(unsafe_code)]` is load-bearing.

**The artifact was not reproducible, twice, for the same reason in two places.**
`load` was a reader, and later `signatures_for` became another: generating an artifact
read the previous one, and a table read back installs no nodes where parsing it would
have, so every node id after it shifted. There is one switch now,
`builtin::generating`, and every reader goes through `artifact()` — so a reader added
later cannot reintroduce it. `tests/check_artifact.sh` caught it both times, which is
the argument for a gate that REGENERATES rather than one that trusts.

### What is left of a `Dict` program

```
[time]            parse    0.058ms   <- the user's four lines
[time] builtin bytecode    0.133ms   <- 99 chunks materialised, not compiled
[time]    builtin::load    0.018ms
[time]       type check    0.104ms   <- including Dict's 36 signatures, read on demand
[time]          compile    0.093ms   <- the app only
[time]              run    0.041ms
```

**0.46ms in process, against 3.5ms when this document was rewritten.** Nothing left is a
re-derivation: the two largest numbers are materialising bytecode that cannot be shared
without `unsafe`, and type-checking the user's own program.

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

### Done instead of the lexer: the TYPE parser — **−6.6% on a `Dict` program**

This phase said the way forward was to tokenize once instead of probing at 269
`starts_with` sites, on a finding of "~200ns per token spread evenly, no hot spot".
**The premise does not hold.** Same line count, different content, 40,000 lines each:

| what each line is | cost per line |
|---|---|
| `f0 = 0 + 0` | 1.11µs |
| `h0 = \|x\| x + 0` | 1.85µs |
| `f0 : I64` | **0.48µs** |
| `f0 : I64, Str -> Try(List(U64), [Bad(Str)])` | **3.00µs** |

An annotation with a real type costs six times the same annotation with a bare one, and
`Builtin.roc` is a file of annotations — a four-line `Dict` program parses 1,358 lines
of it. The cost is `parse_type` and what it ALLOCATES, not the lexing: a counting
allocator put `Try(List(U64), [Bad(Str)])` at **48 heap allocations**.

None of what followed was a rewrite.

**The allocations, in order of what they were worth:**

- `builtin_type` took its arguments by value, so its one caller cloned — a deep copy of
  every argument type for every type atom, thrown away whenever the name was not a
  builtin. It borrows now.
- `claim_intrinsics` deep-cloned a `Type` per intrinsic into a list that **nothing reads
  it from**: every reader of `intrinsics` takes the name. It is a list of names now.
  The low-level section is 454 lines of nothing but intrinsics.
- The type-name scan allocated a `String` per atom to ask whether it contained a dot.
- `named_type` built its fallback with `unwrap_or`, so a `String` and a `Box` per atom,
  discarded whenever `builtin_type` answered.
- `Type.method` was formatted and leaked twice for the same text.
- The `where [` window was "the next 400 characters", and finding where 400 characters
  end means decoding 400 characters — on every annotation, looking for something almost
  none of them have. It is this line and the next now, which is what the comment above
  it already said the rule was. **15.8% on its own.**

**And then the structural half: a type's names are interned.** `types::Type` named every
record field, union tag and nominal with a `String`, which cost twice — an allocation
per name while parsing, and a copy of every name on a clone. A `Type` is cloned
constantly: the parser deep-copies a nominal's whole type on every reference to it.
Measured at 0.69µs per reference to a nominal over a five-field record. All three fields
are `&'static str` from `memory::string_pool` now.

That was 189 construction and match sites, 131 of them in the checker — and every one a
compile error until it was answered, which is what made a change that wide tractable at
all.

| | |
|---|---|
| nominal references | **−33.2%** |
| `Try(List(U64), [Bad(Str)])` | −11.3% |
| `{ x: I64, y: Str }` | −10.4% |
| 40,000 annotations | 137ms → 84ms, **−39%** |
| a `Dict` program, end to end | **−6.6%** |
| `builtin::load` | 1.951ms → 1.821ms |

### The tokenizer is still not written, and is no longer obviously next

Nothing here touched the lexing. The per-token probing is real, but it was never what
`Builtin.roc` was spending its time on, and the table at the top of this section is why
the estimate of "perhaps 40% of parse time" was aimed at the wrong thing. If the
tokenizer is ever written it should be measured against the annotation-heavy case first,
because that is the case that matters to every program that names a `Dict`.

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

### 4.2 — `Dec` arithmetic — **measured and rejected**

`BinDec`/`BinDecK`, exactly as this phase asked for: the checker already knows which
binops are `Dec` (`dec_binops`, beside `integer_binops`), and the opcodes skip
`apply_binop`'s `Int` probe, two-arm `U128` probe and `as_dec` on both operands.

It works, and it is a net loss:

| | |
|---|---|
| `iter_range` | **−5.6%** |
| `records` | **+3.3%** |
| `records_tail` | **+2.7%** |
| `list_pass` | +1.9% |

`records` and `records_tail` execute **not one `Dec` operation**. Their bytecode is
unchanged. Two more arms in `exec` move the dispatch loop's code layout, and that is
bigger than the win — the same finding 4.4 and 4.5 landed on, now for the third time.

Two cheaper shapes were tried and neither pays:

- **Reordering the probes inside `apply_binop`** so `Dec`-and-`Dec` is tested before
  `U128`: `iter_range` +1.3%. The win was never the probe chain.
- **Inlining the `Dec` case into the existing `Bin` arm** (`bin_any`, no new opcode and
  so no new arm): `iter_range` −1.9%, `loop` +2.2%, `matching_tail` +1.7%. A wash.

So the win is real and unreachable: it needs a dedicated arm, and a dedicated arm costs
more elsewhere than it earns. **Ask again only if `exec` stops being layout-sensitive**
— which is its own piece of work, and probably means a computed-goto or a table of
handlers rather than one giant `match`.

### 4.3 — `Move` was the most executed opcode — **done, −17% to −26%**

This entry used to guess that the 15ns an opcode went on drop glue for a 48-byte `Value`.
Counting first said otherwise. A throwaway build with a histogram of executed opcodes:

```
matching   Move 25.0%  TestTag 15.0%  BinInt 15.0%  LoadK 10.0%  Ret 7.5%
records    Move 25.0%  GetField 16.7%  LoadK 8.3%   Ret 8.3%
loop       LoadK 20%   Move 20%   IterNext 20%   BinInt 20%   Jump 20%
calls      BinInt 29.4%  LoadK 23.5%  Move 11.8%  Ret 11.8%  CallFn 11.8%
```

`Move` is the most executed instruction in the interpreter, and `loop`'s five-instruction
body had two that did nothing useful:

```
IterNext { dst: 4, … }
BinInt   { dst: 5, a: 1, b: 4 }   total + i, into a temporary
Move     { dst: 1, src: 5 }       the temporary into `total`
LoadK    { dst: 5, k: Unit }      the statement's value, which nothing reads
Jump
```

**`Compiler::discard`** compiles an expression for its effect. A loop body is a block whose
tail is `{}`, and compiling that tail as an expression materialized `Unit` into a dead
register once per iteration. Only the materialization is skipped; anything that is not a
bare `{}` still runs.

**`Compiler::wrote_directly`** is the destination hint: rather than computing into a
temporary and moving, patch the instruction that produced the value to write the target.
It is called from `assign` and from `values`, and `values` is every place a run of
consecutive registers is filled — a call's arguments, a list's elements, a record's fields,
a tag's payload.

Two conditions make it sound, and both were needed:

- **A straight-line run only.** In a sequence with no `Jump`, `Test*`, `Ret` or `TailCall`
  in it, the last write to a register is the only one that reaches the end. With a branch
  it is not: `total = if c { 1 } else { 2 }` ends with one arm's write, and redirecting
  only that one leaves the other arm writing a register nobody reads. `branches()` lists
  the control-flow opcodes explicitly rather than looking for a `to` field, so a new
  jumping opcode has to be classified on purpose.
- **A temporary only**, identified by `next_reg` from before the value was compiled.
  Anything below it is a live local or an argument already in place, and patching a
  local's write would leave the local unwritten.

`CallFn`, `Call` and `DispatchMethod` are deliberately not redirectable: their `dst` is
written after a frame starting at `base` has been torn down, and pointing it at a live
local would need that overlap reasoned about.

| | executed opcodes | wall |
|---|---|---|
| `loop` | −40% | 11.6ms → **8.6ms** |
| `matching` | −15% | 25.9ms → **20.7ms** |
| `records_tail` | | 10.9ms → **8.8ms** |
| `matching_tail` | | 28.2ms → **23.4ms** |
| `records` | −17% | 11.5ms → 10.7ms |
| `calls` | −12% | 6.7ms → 6.1ms |

Nothing regressed; `iter_range` is unmoved because it is the lazy path, which has no
`Move` to remove.

**What is left of the original guess.** `Value` is still 48 bytes and a register write
still runs drop glue. That may well be the next 15ns, but it is now a *smaller* share than
it looked, because a fifth to a quarter of the writes are gone. Measure again before
shrinking `Value` — the histogram is ten lines and worth rebuilding whenever this section
is reopened.

### 4.4 — `DispatchMethod`'s table lookup — **measured and rejected**

The claim was that resolving a method per call is expensive: a `module_for`, a `HashMap`
lookup on a `(&str, &str)` key, and on a miss a scan of `methods_by_name` with a shape
check per candidate. An inline cache was "the standard fix".

Counted first. `DispatchMethod` is 0% of every benchmark except two — 20% of `list_pass`
and 16.7% of `strings` — so those are the only places it could pay. Then the lookup was
removed outright for the case those two are in: a program that mentions no `Dict`, `Set` or
nominal loads no `Builtin.roc` member, so `methods` and `methods_by_name` are both **empty**
and the hash is paid only to be told so. Skipping it entirely is strictly more than an
inline cache can win.

It won nothing. Interleaved A/B against a freshly built reverted binary, median of 21:

```
                reverted   skip-the-lookup
records            10.8ms       11.5ms      <- 6.5% WORSE
strings             6.1ms        6.2ms
list_pass           3.9ms        3.8ms
matching           20.8ms       20.2ms
records_tail        9.0ms        8.9ms
```

So the lookup is not the cost, and an inline cache cannot beat zero. Reverted.

### 4.5 — `GetField` by slot — **measured and rejected**

`GetField` is 20% of `records`'s instructions and 15.4% of `records_tail`'s, which is a real
number, and it does look up a name: `program.chunks[chunk_id].names[name]`, then a scan of
the record's fields comparing `&str`.

Making that lookup free changed nothing. The cheap half of the idea — hoisting `consts`,
`names` and `pats` into locals of the interpreter loop beside `code`, so none of the 20
sites that read them re-indexes `program.chunks[chunk_id]` — measured as **no win at all**,
including on `records`. LLVM was already hoisting the chunk lookup where it mattered.

That is evidence about the premise, not just about that patch: if reaching the name costs
nothing, the name is not what `GetField` spends its time on. The 20% is an instruction
**count**, and what an instruction costs here is the dispatch, the bounds checks and the
`Value` clone — none of which a slot removes. Resolving a field to a slot needs the
checker's record type threaded through the compiler, and it would buy the comparison of a
one-character field name in a two-field record. Retired for the second time, with numbers
this time.

### The finding worth keeping from both

**The `exec` match is layout-sensitive.** Adding one branch to the `DispatchMethod` arm
moved `records` — which executes **no** `DispatchMethod` at all — by 6.5%. A control
confirmed it is not build nondeterminism: the same source built twice gives the same times
to within 2%.

So a change to one opcode's arm can cost more elsewhere than it saves where it is aimed,
and **the whole benchmark suite has to be A/B'd for any VM change, interleaved**, not just
the benchmark the change targets. Alternating the two binaries in one loop is what made
these numbers readable at all — this machine drifts 7% across a few minutes, which is
larger than every effect in this section.

### 4.6 — top-level names are found by linear scan — **done, 94ms → 2.2ms**

Filed as "nothing anyone runs today hits this", with a guess that the checker's env
shared the problem. Measured: the checker is linear and the COMPILER was the quadratic
one.

| declarations | parse | type check | compile |
|---|---|---|---|
| 1,000 | 1.5ms | 0.6ms | 1.8ms |
| 2,000 | 3.0ms | 1.2ms | 4.1ms |
| 4,000 | 6.0ms | 2.3ms | **19.4ms** |
| 8,000 | 12.2ms | 4.6ms | **94.2ms** |

`Tops::func`/`global`/`alias` were linear scans, and the top level looks EVERY binding
up as it compiles it: 8,000 declarations is 32 million string comparisons. Three
`HashMap`s, built once after the last name is known — `or_insert` and not `insert`,
because a duplicate resolved to the FIRST one when these were scans.

```
8,000 declarations: compile 94.2ms -> 2.21ms, and linear
    1,000 0.36ms   2,000 0.77ms   4,000 1.07ms   8,000 2.21ms
```

No benchmark moves, because nothing in `tests/bench` has more than a handful of
top-level names. That is why it sat here as a ceiling rather than a cost. It is not a
ceiling now.

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

## Phase 6 — process startup — **done, and it was never rocflight's**

The question was "what is the ~350µs before `main`" — relocations, the `Lazy` statics,
or faulting in 700kB of `Builtin.roc`. It is none of them. It is the dynamic loader, and
it costs the same for a program that does nothing:

| | wall, min of 41 |
|---|---|
| `/bin/true` | 1060µs (the harness's own fork/exec) |
| `fn main() { println!("hi") }` | 1260µs — **+200µs** |
| the same, `-C target-feature=+crt-static` | **997µs** |
| `rocflight --version` | 1297µs |

**rocflight's own startup on top of the Rust floor is ~20µs.** There was nothing inside
the program to fix, which is why this phase kept reading as "or never".

So the fix is the link. `.cargo/config.toml` sets `-C target-feature=+crt-static` for
x86_64 Linux only; every other platform builds as before. Static glibc costs `dlopen`
and NSS, and rocflight reads files, writes stdout, reads the clock and forks a compiler.

Two things had to move first:

- **`thiserror` is gone.** A proc macro cannot be built for a statically linked target,
  so one derive blocked the whole thing. It was buying three `Display` impls, which are
  now three `write!` calls in `src/error.rs`.
- **`strip = true`** — a further ~30µs interleaved, and 240kB.

```
rocflight --version   1297µs -> 1042µs, two microseconds over /bin/true
```

Everything short gets it back, because a benchmark's wall includes its own startup:
`closure_capture` −13.0%, `list_ops` −11.8%, `list_pass` −8.1%, `calls` −7.8%,
`records_tail` −6.0%, `loop` −5.8%, `strings` −5.2%, `records` −4.2%. `iter_range`
(183ms) does not move, which is the control: the win is per-process, not per-instruction.

### And fat LTO is still paying for itself

The other half of this phase, measured at last. `lto = "thin"` with
`codegen-units = 16` builds in 8s instead of 21s and loses on ten of twelve benchmarks —
`records_tail` +6.8%, `iter_range` +6.0%, `records` +4.4%. Keep it.

---

## Phase 7 — the allocations that are left — **done, −2.7% to −27.5% across the suite**

`Value::List` was put behind an `Rc` because a list is cloned on every register move,
every argument and every return, and a `Vec` clone copies every element. That argument
was never applied to the other two container arms. `Record` and `Tuple` still hold an
inline `Vec`, so they still pay it.

Measured 2026-09-20, release build, this machine, min of 11 runs, 200,000 iterations
each. Every program is the same shape — build one value, pass it to a function in a
loop, read one thing out of it — so the only variable is what the value is:

```roc
app [main!] {}
get = |p| p.x            # or: p, p.0, List.len(p)
main! = |_args| {
	p = { x: 1, y: 2 }   # or the 16-field / 2- and 16-element variants
	var s = 0
	for _i in 0..<200000 { s = s + get(p) }
	echo!(I64.to_str(s))
	Ok({})
}
```

| what is passed | 2 wide | 16 wide | per pass, over the control |
|---|---|---|---|
| `I64` (control) | 25.4ms | — | 0 |
| `List` (already `Rc`) | 61.3ms | 61.9ms | **flat** |
| `Record` | 33.4ms | 62.2ms | 40ns → 184ns |
| `Tuple` | 36.5ms | 62.4ms | 55ns → 184ns |

The list row is the control that matters: it is 36ms above the `I64` baseline because
it calls two builtins per iteration, and it does **not move** between 2 elements and
16. The record and tuple rows are flat in nothing — **+87%** and **+71%** for the same
work on a wider value, about **10ns per field per pass**. That is a `malloc` and a
memcpy of the field vector on every `Move`, every argument and every return, which
Phase 4.3 already established is the most executed opcode there is.

### 7.1 — `Record` and `Tuple` behind an `Rc` — **done**

`Value::Record(Vec<(&'static str, Value)>)` and `Value::Tuple(Vec<Value>)` are now
`Rc<Vec<…>>`, built through `Value::record` / `Value::tuple` as `List` is built through
`Value::list`. `Value` is unchanged at 48 bytes — `Simd` and `Range` were already
setting that — and `value_stays_narrow` still holds.

It was 31 compile errors and every one of them was a *construction* site. Nothing that
only **reads** a record or a tuple changed at all, because `Rc` derefs through to the
`Vec`: `fields.iter()`, `items.len()`, `fields.iter().find(…)` and `sequence()` are all
untouched. That is why a change spread over 89 sites in six files came to 46 lines.

Interleaved A/B against a copied pre-change binary, median of 11 (21 for the two that
came back flat). Interleaved because this machine drifts ~7% over a few minutes, which
is larger than half these effects:

| benchmark | before | after | |
|---|---|---|---|
| `records` | 10.86ms | 9.34ms | **−13.9%** |
| `closure_in_loop` | 10.47ms | 9.21ms | −12.0% |
| `loop` | 8.73ms | 8.17ms | −6.4% |
| `iter_range` | 219.0ms | 209.0ms | −4.6% |
| `calls`, `closure_capture`, `list_ops`, `matching`, `strings` | | | −3% each |
| `list_pass`, `matching_tail`, `records_tail` | | | −1% to −2% |

Nothing regressed. `records_tail` read +1.4% at 11 runs and −1.0% at 21, which is the
noise floor and not an effect.

And on the probes from the head of this phase, which is where the mechanism shows:

| passed to a function | before | after |
|---|---|---|
| 2-field record | 32.3ms | 27.0ms (**−16.5%**) |
| 16-field record | 59.8ms | 26.8ms (**−55.2%**) |
| 16-tuple | 60.9ms | 29.0ms (**−52.3%**) |

A record and a tuple are now **flat in their width**, exactly as a list has been since
round 1: 26.98ms at 2 fields against 26.81ms at 16. That is the whole of the finding,
and the ~13% on `records` is what it is worth on a benchmark that only ever passes a
2-field one.

**`Rc::make_mut` in `UpdateRecord` does not fire, and that is not a bug in the op.**
The plan above claimed `{ ..p, x: … }` over a `var` would become allocation-free. It
does not, and it took a bytecode dump to see why: the compiler never emits an
`UpdateRecord` whose `dst` is its `obj` — across the whole benchmark suite and all 99
golden programs it is `dst: 1, obj: 0` or `dst: 2, obj: 0`, never the same register — so
the source is still live when the op runs and the refcount is already 2. A `dst == obj`
fast path was written, measured to be dead code, and deleted. Worse, liveness alone
would not fix it: in `p = step(p)` the *caller's* binding holds a second handle across
the whole call. Making this fire is ownership analysis, which is 7.5. The `make_mut` is
kept because it is the correct shape and costs exactly what the copy it replaced cost.

### 7.2 — tag payloads: one allocation instead of two, and none for a bare tag — **done**

`Rc<Vec<Value>>` was a pointer to a box holding a `Vec` pointing at a second buffer:
two allocations for a payload that is never changed after it is built. It is
`Rc<[Value]>` now.

The trap the plan called out is real and worth repeating: **`Rc::from(a_vec)` does not
save the allocation** — it allocates the box and memcpies into it, then frees the
`Vec`, which is strictly worse. The win is only there if the payload is never a `Vec`,
so the 113 `Value::tag(…, vec![…])` call sites became arrays and `MakeTag` collects
straight out of the register window, which for an exact-size iterator allocates once.
`Value::tag` takes `impl Into<Rc<[Value]>>`, so an array, a slice and a `Vec` all still
compile — only the array shape is free.

A tag with no payload is `Value::bare`, cloning one thread-local empty `Rc<[Value]>`.
Thirty-nine sites were writing `Rc::new(Vec::new())`, which allocates: the `Vec` does
not, the `Rc` box does. `Try`'s `map_ok`/`map_err` and `on_err` hand the same `Rc` back
rather than copying the payload.

`matching` −5.0%, `matching_tail` −3.8%, and the two probes at the head of this phase
−2.3% and −3.2%.

### 7.3 — builtin arguments — **done, and the plan was wrong about where they were**

This one was written as "a `Vec` per `CallBuiltin`", with the counting allocator named
as the thing to run before writing the fix. Running it first is what stopped a
4,294-line refactor of the wrong function.

**Builtin argument vectors are 2 to 4 per program on ten of the twelve benchmarks.**
Phase 5 lowered the callback builtins into in-frame loops and most arithmetic is
`BinInt`, so `CallBuiltin` is simply not where a program's time goes any more. The two
exceptions had 12,006 (`list_pass`) and 8,003 (`strings`) — and those were not
`CallBuiltin` either. They were **`DispatchMethod`**, which collected the register
window into a `Vec`, `remove(0)`'d the receiver off the front — an O(n) shift — and
then built a *second* `Vec` to put the receiver back at the front, because that is the
shape `call_builtin_values` wants. Two allocations and a shift on every `xs.len()`.

Both are gone. `call_builtin_values` and `dispatch_builtin` take `&mut [Value]`, which
**is** the register window, receiver at slot 0. The plan's claim that "nearly every
builtin destructures an owned `Vec`" was also wrong: the dispatch layer only ever read
`&args`, exactly seven functions took one by value, and each of those immediately did
`let mut args = args;` and `mem::replace`d elements out of it — which is what
collecting the window did anyway. Those registers are dead after the call either way,
so handing over the window changes nothing but the `malloc`.

| | allocations before | after |
|---|---|---|
| `list_pass` | 34,217 | 14,210 |
| `strings` | 41,806 | 25,803 |

`list_pass` −7.3%, `list_ops` −4.2%. No `SmallVec` and no pool: the register file was
already the stack-shaped place these values live, and the honest version of "stack
instead of heap" was to stop copying them out of it.

### 7.4 — the one-liners — **done**

`Chunk::params` and `Closure::params` are `Rc<[&'static str]>`. `MakeRecord` takes each
field out of its register as `MakeTuple` and `MakeTag` beside it do — the compiler
resets `next_reg` to `base` before allocating the destination, so those registers are
dead and cloning them was copying a nested record for nothing.

### 7.5 — the last read takes the value — **done, −11.4% and −9.8%**

`Rc` made a record cheap to SHARE. It never made one cheap to CHANGE. `Op::Move`
cloned, so `p = step(p)` left the caller's binding holding a second handle for the
whole call, and `Rc::make_mut` in `UpdateRecord` always copied.

**Confirmed before it was written.** A throwaway build that took unconditionally —
unsound in general, fine for one program — turned `records.roc`'s 80,720 allocations
into 719 with the answer unchanged. That said the design was right, and it bounded what
a sound version could be worth before any of it was built.

`src/vm/liveness.rs` is a backward liveness fixpoint over the flat opcode vector. Where
it proves a read is the last one, `Move` becomes `MoveTake` and `UpdateRecord` gets
`take: true`; the value moves out of its register, the refcount falls to one, and
`make_mut` mutates in place.

**The soundness rule is one-directional, and it is the whole reason this is safe to
ship:** reads may be over-approximated and kills under-approximated, never the reverse.
An extra read or a missed kill only costs a take. A *missed* read would empty a register
something still needs and answer wrongly, with no crash. So `reads`, `kills` and
`successors` each match every opcode by name with **no wildcard arm** — a new opcode
does not compile until someone has said what it does. `GetFieldOr`, `IterNext` and
`TestStr` write only on one of their outgoing edges, so they kill nothing.

`TailCall` is the one range kill, and it is not a detail: it moves its arguments down
onto registers `0..argc` before looping back to the top, so a tail-recursive function's
parameters are **not** live round the loop. Without that kill `records_tail` got no
takes at all and read **+6.9%** from code layout alone — a regression bought with no
win. With it, −9.8%. A separate `MoveTake` opcode rather than a flag on `Move`, because
`Move` is the most executed instruction there is and this VM's dispatch is measurably
layout-sensitive.

| | before | after | allocations |
|---|---|---|---|
| `records` | 9.44ms | 8.36ms (**−11.4%**) | 80,720 → 828 |
| `records_tail` | 9.11ms | 8.22ms (**−9.8%**) | 80,575 → 674 |
| `loop` | | −3.0% | |

`ListPush` and `Lazy::advance` share the same `make_mut` and will benefit wherever the
analysis reaches them; nothing was measured there and nothing is claimed.

### What Phase 7 came to

All of it, against the pre-7.1 binary, interleaved, median of 15:

| | | | |
|---|---|---|---|
| `records` −27.5% | `matching_tail` −10.4% | `list_pass` −10.2% | `records_tail` −10.6% |
| `matching` −9.7% | `list_ops` −7.8% | `strings` −6.2% | `closure_in_loop` −5.6% |
| `iter_range` −5.3% | `closure_capture` −5.1% | `loop` −2.9% | `calls` −2.7% |

Every benchmark faster, none regressed, 1953 of 1953 throughout.

### Measured and rejected

- **`Box<Expr>` → `Rc<Expr>` in the AST.** `Rc` pays for itself only where something is
  *shared or cloned*, and nothing clones the AST: one `grep` for a clone of an `Expr`
  across the whole crate finds a single site, `compile.rs:1754`, which builds a
  desugaring's callee once. `Lambda` already holds `Rc<Expr>` because a closure value
  genuinely does share its body. Every other `Box<Expr>` is a unique child of a unique
  parent, and `Rc` would add a refcount to each one and save nothing. The AST's real
  cost is the **number** of small allocations, not their kind — see
  [Not attempted: AST node allocation](#not-attempted-ast-node-allocation), which is an
  arena question and is still the right framing.
- **`Box<Type>` → `Rc<Type>` in the checker.** This one *looks* compelling:
  `checker.rs` has 135 `.clone()` sites, thirteen of them the literal
  `(**backing).clone()` / `(**inner).clone()` deep copy of a type tree, and
  `Type::Record { fields: Vec<(String, Type)> }` allocates a `String` per field per
  clone. Then the probe answers the question: `ROCFLIGHT_TIME=1` on a trivial program
  reads **0.048ms for the whole type check**. Phases 1 and 3 took the front end from
  3.5ms to ~0.17ms end to end, and the checker is now a rounding error. There is no
  phase here, only a tidier data structure, and this document does not trade a 4,277-line
  refactor for tidiness. Re-open it only if something makes the checker hot again.
- **An inline small-string for `Value::Str`.** `Value` has 32 usable payload bytes after
  `i128` alignment, so a short string could live inline and every short concatenation
  and interpolation would stop allocating. It is also a second representation of a
  string threaded through every `Str.*` builtin in `eval/mod.rs`, and `Rc<str>` already
  makes the *clone* free — only the *construction* allocates. Measure `tests/bench/strings.roc`
  against a counting allocator before anyone writes a line of it.

---

## Phase 8 — instructions, now that the allocations are gone — **done, −1.4% to −20.8%**

Phase 7 took the allocations out. What was left is dispatches, so the question became
"which instructions does a program actually execute", and the answer came from an
**opcode histogram** — a throwaway build, ten lines counting in `exec`, never kept,
because counting in the dispatch loop costs what it measures. Rebuild it whenever this
area is reopened; it is what picked all three of these, and none of them was the item
this document had queued next.

| | `loop` | `records_tail` | `calls` | `iter_range` | `records` |
|---|---|---|---|---|---|
| `Jump` (a loop's back edge) | 33.3% | — | — | 25.0% | 10.0% |
| `LoadK` (a literal operand) | — | 30.8% | 26.7% | — | — |
| `Move`/`MoveTake` | — | — | — | 25.0% | 20.0% |

Those three rows are 8.1, 8.2 and 8.3.

### 8.1 — a `for` loop's back edge does the step itself — **done**

The compiler lays a loop out as `IterNext { to: exit }`, the body, `Jump { to: head }`,
`exit:`. The `Jump` exists only to reach the `IterNext`, so `vm::peephole` replaces it
with `IterNextBack`, which jumps to the BODY when there is an element and falls through
when there is not. All three facts it relies on — back edge, target is an `IterNext`,
`exit == jump + 1` — are checked, and a `Jump` something else targets is left alone
because a `continue` lands there and must still mean "go round again".

A rewrite **in place**: no instruction added or removed, so nothing is renumbered. That
is why this fusion exists and a more general one does not.

`IterNext` and `IterNextBack` share `iter_step`, and **`#[inline(always)]` on it is
load-bearing**: the first cut left it to LLVM, it stayed out of line, and `iter_range`
read **+10.8%**. Same trap as the `Lazy::advance` split. `loop` −11.8%, `iter_range`
−4.7%, `records_tail` −5.5%.

### 8.2 — a literal operand is read from the constant table — **done**

`x + 1` was a `LoadK` into a register and a `BinInt` reading it back: a whole dispatch,
a 48-byte copy and the drop glue on whatever the register held. `BinK`/`BinIntK` take
the constant index in place of the second register.

The compiler folds them when the last instruction emitted is a `LoadK` into the right
operand **and** that register is a temporary the operands' own compilation allocated
(`b >= save`, the `next_reg` watermark from before they compiled). The second test is
the whole safety argument:

```roc
y = 5
x + y
```

matches the first test — `y`'s own `LoadK` is the previous instruction and `y`'s
register is the operand — and fusing there deletes the binding, leaving every later read
of `y` empty. Verified by hand on exactly that program.

The `LoadK` is **replaced**, not removed, so nothing is renumbered — which matters,
because a `while` loop's head is the first instruction of its condition and for
`while i < 10` that *is* this `LoadK`. The back edge now lands on the fused instruction,
which is still where the condition starts. The span moves to the operator so an overflow
reports at the operator.

`BinInt`'s body moved into `bin_int` so `BinIntK` cannot drift from it. `wrote_directly`
had to learn the two new opcodes or it put the `Move` back — `go` was 13 instructions
per iteration, then 12, and only then 10. `records_tail` −15.6%, `calls` −15.4%,
`closure_in_loop` −14.3%.

### 8.3 — the destination hint reaches calls and the lowered loops — **done**

Phase 4.3's hint never reached two places, and once 8.1 and 8.2 had gone they were what
was left.

`wrote_directly` listed `CallFn`/`Call`/`DispatchMethod` as deliberately absent, because
their `dst` is written after a frame starting at `base` is torn down and the overlap had
not been reasoned about. It has been: redirecting onto a local is safe exactly when the
local sits below that frame, and arguments are always allocated above every live local
— so it always does, and the code checks `dst < arg_base` rather than arguing it.

`finish_element` emitted `Move { dst: accumulator, src: body }` unconditionally; the
lowered loops arrived in Phase 5, after 4.3 was written, and never got the hint. Now the
body's last instruction writes the accumulator.

`iter_range`'s loop body is **two** instructions where it was four, `records`' is three
where it was five. `iter_range` −8.7%, `loop` −4.9%, `records` −4.3%. `calls` reads
+0.9% and stays there at 25 runs; its bytecode was diffed and the only change is one
instruction removed from `main!`, which runs once — layout, not work.

### What Phase 8 came to

Against the pre-phase binary, interleaved, median of 15:

| | | | |
|---|---|---|---|
| `calls` −20.8% | `records_tail` −17.9% | `iter_range` −13.5% | `loop` −13.2% |
| `closure_in_loop` −11.5% | `records` −8.6% | `matching_tail` −7.2% | `closure_capture` −5.1% |
| `list_ops` −3.9% | `strings` −3.9% | `list_pass` −2.0% | `matching` −1.4% |

Every benchmark faster, none regressed, 1953 of 1953 throughout. `loop.roc` is now two
instructions per iteration and `iter_range` two; there is no third to remove.

### What the histogram says is left

Re-measured after all three. `matching` is `TestTag` 18.2% and `BinInt` 18.2% — a match
arm's test and its arithmetic, both real work. `list_pass` is `DispatchMethod` 21.4% and
`Move` 21.4%, and those Moves are argument setup for a receiver read three times, so
only the last can be a take. `records` is `GetField` 22.2%, which Phase 4.5 already
measured is an instruction count and not a lookup cost.

There is no obvious fourth fusion. The next question about the VM is the per-op floor
itself — see Phase 4.

---

## Where the time goes now — **every phase is done**

A four-line program that names a `Dict`, with its own fork/exec taken off: **1.14ms**,
against 3.5ms when this document was rewritten. Nothing in it is a re-derivation any
more — the front end reads trees instead of building them, and startup is the operating
system's.

```
parse 0.077ms | builtin::load 0.339ms | type check 0.058ms | compile 0.655ms | run 0.052ms
```

`compile` is the largest single number left and it is the same question in a new place:
every definition `Builtin.roc` declares is compiled whether the program calls it or not.
See the end of Phase 2.

### The gates, and which one is load-bearing

```bash
cargo build --release              # refuses a stale Builtin.artifact
tests/check_eval.sh --strict       # 1953 of 1953 — the hard requirement
tests/check_roc.sh --strict        # 99 golden pairs
tests/check_examples.sh            # 20 matching examples
tests/check_artifact.sh            # the artifact regenerates byte-identical
tests/check_host.sh                # the platform host, built for musl
cargo test --quiet                 # the Rust suite
tests/bench.sh                     # A/B, interleaved, against a copied binary
```

**The eval suite is the one that catches parser changes.** Two refusals in this document
were found by it while `check_roc.sh` and `check_examples.sh` both stayed green — the
`skip_trivia` guard (1881 of 1953) and narrowing `needed_by` (1949 of 1953).

### The one cheap shortcut past the parser, refused

`skip_trivia` runs between every token and asks `capture_nominal_declaration` and
`capture_type_annotation`, each of which scans to the end of the line and searches it.
Both constructs read to the next `\n`, so guarding them on "the cursor is at the start
of a line" looks free, and it is worth **−4.2%** on a `Dict` program.

It reads **1881 of 1953**. Annotations reach `skip_trivia` mid-line often enough to
break 72 eval tests — and `tests/check_roc.sh` (99 golden pairs) and
`tests/check_examples.sh` both stayed GREEN while they did. That is the sharpest
reminder in this document of which gate is load-bearing.

### The VM's per-op floor — measured, and there is nothing to shrink

`loop.roc` is two instructions per iteration at ~18ns each. The standing theory was drop
glue on every 48-byte register write, and that shrinking `Value` would pay. It cannot
be shrunk: **three variants force 48 bytes independently** — `Simd { u8, u128 }`,
`Range { i128, i128, bool, i64 }` and `Tag(&'static str, Rc<[Value]>)`. Getting to 32
means boxing `Simd` (fine), boxing `Range` (an allocation per loop, which Phase 4.1
spent its effort removing) and putting `Tag`'s payload back behind a `Vec` (undoing
7.2). There is no version of this that wins.

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
8. ~~**Phase 4.3**~~ — done: `Move` was the most executed opcode, and a destination hint
   plus not materializing a discarded `Unit` took `loop` −26%, `matching` −20%,
   `records_tail` −19%. **4.4 and 4.5 measured and rejected** — neither's premise held, and
   perturbing the `DispatchMethod` arm cost `records` 6.5% through code layout alone.
9. **Phase 2** — see 14.
10. ~~**Phase 7**~~ — done, all five items. Every benchmark faster, none regressed,
   1953 of 1953 throughout: `records` −27.5%, `matching_tail` −10.4%, `list_pass`
   −10.2%, down to `calls` −2.7%. Three of its own claims were wrong and the
   measurements are what caught each one — 7.1's `make_mut` (the compiler never emits
   `dst == obj`), 7.3's whole premise (the `Vec`s were in `DispatchMethod`, not
   `CallBuiltin`, and there were four of them per program elsewhere), and 7.3's "nearly
   every builtin destructures an owned `Vec`" (seven functions did, and none of them
   needed to). 7.5 is the one to be careful around: a liveness pass whose failure mode
   is a wrong answer with no crash, kept safe by matching every opcode with no wildcard
   arm.
11. ~~**Phase 4.6**~~ — done, and it was the COMPILER that was quadratic, not the
   checker: compiling 8,000 declarations went 94.2ms → 2.21ms and is linear now.
   ~~**Phase 4.2**~~ — measured and rejected, the third time code layout has beaten a
   new opcode. ~~**Phase 1.3b**~~ — done, −12.7% on a `Dict` program, and it turned one
   PENDING example into a passing one.
12. ~~**Phase 6**~~ — done, and the answer was that the startup was never rocflight's:
   ~200µs of it is the dynamic loader, which a static link removes. `--version` is now
   two microseconds over `/bin/true`. Fat LTO measured and kept.
13. ~~**Phase 3**~~ — done, and it was not the lexer. The premise was wrong: an
   annotation with a real type costs six times a bare one, and `Builtin.roc` is a file
   of annotations, so the cost was `parse_type`'s allocations. −39% on annotation-heavy
   parsing, −6.6% on a `Dict` program, and `types::Type`'s names are interned. The
   tokenizer is still unwritten and is no longer obviously next.
14. ~~**Phase 2**~~ — done twice over. Parsed at build time: `builtin::load` 1.821ms →
   0.339ms, −39.7%, with the strings read out of the binary's rodata rather than
   allocated. Then COMPILED at build time: `compile` 0.604ms → 0.088ms, another −56%.
   A `Dict` program is 0.49ms in process against 3.5ms when this document was rewritten.
   A stale artifact is a build error.
15. ~~**Phase 8**~~ — done, all three items, and an opcode histogram picked every one
   of them over what this list had queued: a `for` loop's back edge, a literal operand,
   and Phase 4.3's destination hint reaching calls and the lowered loops. `calls`
   −20.8%, `records_tail` −17.9%, `iter_range` −13.5%, nothing regressed. `loop.roc` is
   two instructions per iteration now.


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
