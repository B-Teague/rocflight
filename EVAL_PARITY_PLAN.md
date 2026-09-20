# Eval parity plan

How rocflight gets from **1,736 of 1,953** to all of them, on roc's own eval tests, as
one backend of roc's own runner. The gate is `tests/check_eval.sh`; done is
`tests/check_eval.sh --strict` green. Every number here was measured on 2026-09-18
against roc-compiler `697ada7b` (Sep 11) with the runner patched as described in
`TESTING_STRATEGY.md`, from `tests/check_eval.sh --keep --report`. Re-measure before
trusting any of them.

The suite is 2,299 `TestCase`s in `roc-compiler/src/eval/test/`. The runner excludes
212 opt-in proof cases by default, leaving 2,087. Of those, 1,953 run a backend
(`inspect_str`, `allocations_at_most`, `crash`), 71 expect a compile problem, and 83
check compile-time float bits with no backend at all.

**Where it stands.** Phases 0 to 7 (the first plan, 2026-09-17 to 18) took the tally
from 957 to 1,736, the refused problem tests from 22 of 71 to 49, and the hangs from 2
to 0. They are summarised in "The first plan" at the end. What is left is **217 backend
tests and 22 problem tests**, and this document is the plan for those: phases 8 to 17,
each named for the family of tests it turns green. **As of 2026-09-19: 1,936 of 1,953,
65 of 72 problem tests refused.** Phases 8, 9, 14, 15, 18, 19, 21, 22 and 23 are done;
12, 20 and 24 mostly; 10, 11, 13, 16 and 17 partly. What is left is 17 backend tests and
7 problem tests: phase 25 (libm and the one-offs), the three refusals phase 24 measured
and reverted, and the two Phase 20 tests that need runtime integer widths. The 62 backend tests and ~16 problem tests still open no longer
split by feature — they split by a handful of shared mechanisms rocflight has never
had, which are **phases 18 to 25** ("The remaining residue" below): nominal identity at
run time, parameterized nominals, value-level monomorphization, capturing nominal
methods, a `Builtin.roc`-shaped `Iter`, the JSON codec protocol, the last checker
refusals, and libm bit-exactness. Type-level monomorphization — carrying a concrete
type into a generic body at the checker — already landed across phases 10 and 11.

## Where the 239 come from

Every remaining failure, by family. A test is counted once, under the family whose
fix it needs first; the families were assigned by name and confirmed by reading the
kept sources and stderr under `target/eval-fails/`.

| Tests | Family | Phase |
|---:|---|---|
| 35 | `from_numeral` / `from_quote` / `from_interpolation` nominals, literal patterns through `is_eq` | 8 |
| 22 | lazy `Iter` semantics, `Range.custom`, `Dec` and float ranges, `while` + tuple `var` | 9 |
| 26 | generic dispatch and cross-module methods | 10 |
| 14 | optional and defaulted record fields | 11 |
| 22 | 128-bit integers, overflow crashes, bit-exact floats | 12 |
| 17 | `Set` / `Dict` structural keys and custom codecs | 13 |
| 18 | crypto: `SHA256` and `BLAKE3` | 14 |
| 39 | SIMD vector types | 15 |
| 22 | problem tests rocflight still runs | 16 |
| 24 | one each: the long tail | 17 |

By what rocflight said, from the kept stderr:

| Count | rocflight said | Meaning |
|---:|---|---|
| ~120 | `Type error … does not have it` / `Cannot dispatch … on an unresolved type` | the checker cannot name the receiver's type: a nominal built from an imported module, a numeral flowing into a nominal with `from_numeral`, a `U128` method on a numeral, a SIMD or crypto type it has never seen |
| ~45 | `Unknown function M.m` | a builtin that does not exist yet: every SIMD and crypto method, `Iter.custom`, `Range.custom`, `Numeral.*` |
| ~35 | a different value | `size_hint` answers `Known` for `Unknown`, `U128` above `i128::MAX` wraps negative, plain `I128` add does not crash, `Set.fold` folds a `Dict`, libm bits differ |
| ~15 | `Runtime error` | `.?` chains reading through an `Ok`, `Dict` keys that are records, `Set` of zero-sized items |
| 22 | (ran the program) | a problem test: roc refuses it, rocflight does not |
| 1 | (94 s, then failed) | issue 8729: `var` reassigned through a tuple pattern in `while` still spins |

## The phases

Each phase names the tests it turns green, the change, and its check. A phase is done
when its `--filter` is green and the three other gates are unchanged:
`cargo test`, `tests/check_roc.sh --strict`, `tests/check_examples.sh`, `tests/bench.sh`.
Filters are substrings of test names, matched by `tests/check_eval.sh --filter`, and
several `--filter`s union.

### Phase 8: `Numeral`, `from_numeral`, `from_quote`, `from_interpolation` — DONE

*Turned green: 33 (1,736 → 1,769); its filter is 54 of 55.* What changed:

- **A `Numeral` value** (`src/eval/numeral.rs`), built from the literal's text — the
  parser now keeps every numeric literal's digits — or from the value where the text
  is gone. `Numeral.is_negative`, `digits_before_pt`, `digits_after_pt`,
  `digits_after_pt_count` read it; `U32.from_numeral` and the other widths are the
  width's `from_str` of the text, failing with `InvalidNumeral(Str)`.
- **The checker routes literals.** A numeral unifies with a nominal that declares
  `from_numeral`. A string literal is polymorphic — a `Str` or a nominal with
  `from_quote` / `from_interpolation` — only in a program that declares one, so every
  other program pays nothing. `literal_conversions()` names each literal's nominal and
  method, suffixes included (`123.MyNum`, `'a'.Code`, `"Roc".Tag`, a multiline
  string's `.Tally`, `"a${b}".Url`); the compiler emits the call and unwraps the `Try`
  (`from_interpolation` returns what it declares, so `url : Try(Url, e) = "…${d}…"`
  keeps the `Try`).
- **Literal patterns.** A numeral pattern is a numeral, so `380 =>` against a `Px`
  converts through `Px.from_numeral` and compares with `Px.is_eq`, structurally when
  the equality is derived (`is_eq : _`). Each `match`'s scrutinee type reaches the
  compiler, which threads it through tuple, list, tag and record patterns; against a
  type it does not know (a `where` helper's parameter) `TestLitDyn` asks the value's
  shape at run time. A numeral pattern also matches a `Dec` scrutinee now that both
  default together.
- **Generic bodies.** roc specialises `go = |f| f("hello")` per call site; rocflight
  converts at the boundary instead: a parameter or binding declared as a conversion
  nominal runs `Lit.coerce` on a raw literal that arrives (a `Str` or a number, also
  inside a `List` or an `Ok`), and passes anything else through. That is the three
  `imported polymorphic` regressions, `generalized from_quote` / `interpolation`, and
  `from_quote survives a function stored in a record`.
- Also: a nominal's own operator signature types the right operand (`Duration.times :
  Duration, I64 -> Duration`, B046); an interpolation-pattern capture stops at the
  first byte of its delimiter; the direct operator-method call no longer clobbers its
  second argument when both operands are temporaries — it compared `a.map(f)` with
  itself, which is why `Set.algebra and map use set membership` passed before and
  fails honestly now (Phase 13); two nominals of one field list at one depth are told
  apart by their fields' kinds (`{ value : F32 }` from `{ value : F64 }`) or reported
  ambiguous rather than guessed.

Left in this family, 2:

- `custom from_numeral Err in an uncalled function is a compile-time problem`: roc
  runs a literal conversion at compile time, rocflight where the literal is used, so
  `unused = |_| 42` with a rejecting `from_numeral` is never reached. Hoisting every
  literal conversion to program start would close it. It counts as an accepted
  problem test now: 48 of 71 refused, from 49.
- `literal pattern checked equality: local equality captures`: a nominal declared
  inside a function, whose `is_eq` captures a local — the block-local nominal family
  of Phase 10.

The plan as it was:

*Turns green: 35.* Tests: every name containing `from_numeral`, `from_quote`,
`interpolation`, `literal pattern checked equality`, regressions B045 and B046, and
`multiline string literal type suffix on its own line`.

What rocflight does today: a numeral whose expected type is a nominal is a type error
(`Expected: Big, Actual: a number`); `"…".Custom` and `"${x}".Custom` dispatch only to
the builtin string; a literal pattern against a nominal scrutinee compares raw values.
`from_quote` already works for the local, monomorphic case through `quoted()` in
`src/vm/compile.rs`, and that is the template.

The change:

- **A `Numeral` value.** `Builtin.roc` declares it (line ~6547) as
  `Literal({ is_negative, digits_before_pt : List(U8), digits_after_pt : List(U8), digits_after_pt_count : U64 })`
  with base-256 digits. Build it in the parser from the literal's text (the parser
  already keeps the digits exact for `Dec`), and answer `is_negative`,
  `digits_before_pt`, `digits_after_pt`, `digits_after_pt_count` from it. Twelve tests
  read `digits_before_pt`; four read the count.
- **Route numerals through `from_numeral`.** In the checker, when a numeral's type var
  unifies with a nominal that declares `from_numeral`, record the node the way
  `quoted_literals` records `from_quote` targets; the compiler emits
  `CallFn Nominal.from_numeral(numeral)` and unwraps the `Try` as `quoted()` does. The
  same for `123.MyNum` and `'a'.Code` suffixes (the parser records suffixes already) and
  for imported nominals, where the module is the import.
- **`from_interpolation`.** `"${x}"` whose expected type is a nominal with
  `from_interpolation` calls it with the interpolated parts; the `Try` variant forwards
  the error type. Three tests are "imported polymorphic" versions of these: the
  operand the generated call receives must be the numeral or string, not a `Dec`.
- **Literal patterns through `is_eq`.** A literal pattern whose scrutinee is a
  nominal (or a record, tag union, or recursive nominal that derives equality) converts
  the literal as above and compares with the nominal's `is_eq`, so `match big { 12 => … }`
  compares converted values. Guards short-circuit; captured locals in the equality
  method are preserved.

Check: `tests/check_eval.sh --filter from_numeral --filter from_quote --filter interpolation --filter "literal pattern checked"`.

### Phase 9: a lazy `Iter`, `Range.custom`, ranges over `Dec` — DONE

*Turned green: 14 (1,769 → 1,782); its filter is 140 of 149.* What changed:

- **A lazy iterator value** (`Value::Iter`, one `Rc` so `Value` stays 48 bytes) with
  a `Lazy` enum in `src/eval/lazy.rs`: a list or range walked in place, `Iter.custom`'s
  unfold, and `keep_if`/`drop_if`/`map`/`with_index`/`step_by`/`concat`/`take_first`
  wrappers. Each `step` is pure — it returns the item and the REST iterator, and a
  rejected item is a `Skip`, exactly as roc's `next` does. `Iter.custom` is no longer
  eager, so a `fib` unfold under `take_first(5)` terminates; `keep_if` emits `Skip`;
  `size_hint` is `Unknown` after a filter and `Known(a+b)` across a `concat`. The eager
  `List` and integer `Range` paths are untouched, so the benchmark iteration cost does
  not move; the shared method names (`concat`, `size_hint`) go lazy only for a range or
  an iterator, never for `List.concat` of two lists.
- **Ranges over any numeric type.** `Type::Range` now carries its element, so a range
  of literals is a numeral (a bare `(1..=n)` iterates `Dec`s, as roc does) and
  `(1.0..<4.0)` or `(1.U8..=3.U8)` type and run. `MakeRange` keeps two integers as the
  fast `Value::Range` and builds a lazy iterator for `Dec`/float bounds. The four
  `range_*_to` / `range_*_from` constructors are typed and implemented for `Dec` and
  the floats; `_from` reverses. `size_hint` reports `Unknown` when the count overflows
  `U64`.
- **`.iter()` keeps the element type** rather than a fresh variable, and a `Range`
  receiver unifies against `List(element)` in the declared-signature path — without
  this a range's element was thrown away and `(1..=4000).iter().map(double).fold(0,
  add)` folded `Dec`s even though `double : I64 -> I64` should pin it.
- **A custom iterable** — a nominal with an `iter` method — is looped by calling
  `.iter()` on it (at the checker for the element type, at run time in `IterNext`), so
  `for x in mixed` works.
- **A program's own `Iter` record type** wins over the builtin `Iter` (which maps to
  `List`): a nominal name is registered before its backing is parsed, so a recursive
  reference inside resolves to the nominal.

Left in this family, ~7: the three `iterator-like` tests need recursive
record-nominal unification (a user `Iter` whose method returns `{ next: ... }`
referring to itself); `Range.custom` iterating a third-party numeric type through its
`range_iter`; a fractional `step_by(1.5)` on a reversed range; the `Dec`-highest
exclusive-range edge; and `Str.iter_utf8`'s bytes typing (its sum defaults to `Dec`).
They are the tail, not the phase.

The plan as it was:

*Turns green: 22.* Tests: `Iter.keep_if emits skip with rest iterator`, the four
`reports unknown length` tests, `range size_hint reports Known counts …`,
`Range.custom iterates a third-party numeric type`, `recursive custom iterator
take_first works in for loop`, `wrapper iterator with distinct branch producers`,
the three `iterator-like` tests over a user-defined `Iter`-shaped record, the six
numeric-range tests (`inclusive`/`exclusive … all iterate`, `stop at highest`, `from
constructors create ranges in reverse direction`, `collect over a Dec range`), the two
`iter alloc` fold tests, `Set.from_iter skips deduplicates and collects`, `infinite
loop has no synthetic false return path`, and issue 8729.

What rocflight does today: `Iter` is a `List` (`builtin_type` maps `Iter(a)` to
`List(a)`), ranges are materialised except under walkers, and `Iter.next` is `List.get`.
Everything that observes laziness or an unknown length fails on that.

The change:

- **`Value::Iter { state, next, size_hint }`**, a closure-and-state pair as roc's
  `Iter.custom(state, len_if_known, step)` builds it. `next` returns roc's tag:
  `Ok((item, rest))`, `Skip({ rest })`, or `Err(NoMore)`. `List.iter()`, ranges, and
  every existing eager `Iter.*` builtin produce or consume it; `fold`, `for`, `collect`
  and `List.from_iter` drive it. `keep_if` emits `Skip`; `concat`, `step_by`, and
  `with_index` propagate `Unknown` when a side is unknown. The eager path stays for a
  `List` receiver, so the bench does not move; this is the change most likely to, and
  the bench is the check.
- **`Range` over any `Num`.** `MakeRange` keeps `Dec` and float bounds (it already
  accepts a whole `Dec`); step and direction (`from`, descending) follow roc's
  `Range` methods; `size_hint` is `Known(n)`, `Known(0)` descending, `Unknown` when the
  count overflows `U64`.
- **`Range.custom({ lower, upper, step, upper_bound, direction, len_if_known })`**
  calls the element type's `range_iter` method, so a nominal `Distance` with
  `range_iter` iterates.
- **A user-defined `Iter` nominal** (the `iterator-like` tests declare their own `Iter`
  record with `map`/`keep_if`/`drop_if`) must win over the builtin, which `declared()`
  already arranges for the type; dispatch has to follow.
- **Issue 8729** spins for 94 s: the same `var`-through-tuple-pattern fix as Phase 7,
  but the reassignment is inside a `while` condition's body; find it with
  `--filter "issue 8729" -- --timeout 5000`.

Check: `tests/check_eval.sh --filter "Iter." --filter range --filter iterator --filter "iter alloc" --filter "issue 8729"`, then `tests/bench.sh` against the pre-phase binary.

### Phase 10: generic dispatch and cross-module methods — PARTLY DONE

*Turned green: 4 (1,782 → 1,786).* What landed:

- **`|>` supplies the first explicit argument.** `xs |> r.concat()` is `r.concat(xs)`
  and `2 |> h.sum(4)` is `h.sum(2, 4)`; a parenthesized target `2 |> (bar(3).blah())`
  groups instead, applying the piped value to the computed function, and `1 |>
  (|v| v + 1)()` inserts because a call follows the paren. The numeric operator method
  names (`Dec.plus` / `minus` / `times`) were added so `1 |> 1.plus()` works.
- **Imported nominal constructors are nominal.** A bare tag-union receiver whose tags
  belong to a nominal that declares the method IS that nominal, so
  `CounterMod.Counter(41).get()` type-checks; imported modules' nominal shapes are now
  registered for run-time dispatch, so `Helpers.read(CrateMod.Crate(5))` is no longer
  ambiguous.
- **`where`-promised generic dispatch** returns what the enclosing function's declared
  type says rather than the builtin table's guess, so `read : item -> U64 where
  [item.get : item -> U64]` gives `U64`, not the `Try` that `List.get` would.

Still failing in this family (~16), the hard core: a nominal declared **inside a
function or block** cannot see the enclosing scope's locals (`Undefined variable:
offset` / `modulus` / `second`) and two block-locals of one shape are conflated;
**specialization that stays replaceable until constrained** (a numeric default, an
empty list); **F64/F32 specialization** of one comparison; **static dispatch through
an alias re-export** and **tag-union type aliases** keeping their methods (issue 8637);
**mutually recursive nominals** in one module; a **nominal record imported** across
modules; **polymorphic record update** (B098) and a **dispatched imported method
returning a record** (B103). These need the checker to carry a concrete type into a
generic body the way roc's monomorphization does, and block-local nominal scoping —
each its own change. The codec-delegation tests the filter also caught (11063, 11094,
9796) belong to Phase 13.

The plan as it was:

*Turns green: 27.* Tests: `cross-module attached method specialization on imported
nominal`, `literal pattern checked equality: local equality captures` (a block-local
nominal whose `is_eq` captures a local), `cross-module polymorphic attached method specialization from helper module`,
`nominal record imported across modules reads correct fields`, `static dispatch through
alias re-export resolves declaring module (issue 9875)`, `static dispatch receiver
result feeds another method call`, the two `attached methods on … tag union alias
(issue 8637)` tests, `deeply nested associated items (5+ levels)`, the three
`block-local` / `same-named block-local` / `generic dispatch preserves each capturing
local method context` tests, `imported generic dispatch preserves caller local method
target`, the two `where` owner tests (`explicit where method constraint keeps owner
generic`, `imported where helper remains generic when another owner is visible`),
issue 11099 (`where`-clause closure read out of a record in an imported module), the
two `pipe … first explicit method argument` tests, `function-value Str.inspect
preserves nominal method`, `numeric default specialization remains replaceable until
constrained`, `unconstrained empty list specialization remains replaceable until
constrained`, issue 11189 (`F64` and `F32` specializations of one comparison), issue
10049 (structural containers honor component equality and hash), regression B098
(polymorphic record update preserves row fields), regression B103 (dispatched
imported-module method returning a multi-field record), `regression: Dict uses a
custom nominal key hash method`, and `mutually recursive data structures in one type
module`.

What rocflight does today: `CounterMod.Counter(41).get()` fails with `Actual:
[Counter($2), ..]` — the constructor of an imported nominal types as a bare tag, so the
receiver has no methods. Locally declared nominals work because `nominal_literals`
records `Module.Tag(...)`; the import path never reaches it. The `where` and
"specialization" tests fail the other way: the checker pins a generic helper to the
first owner it sees.

The change:

- **Imported constructors are nominal.** `Import.Type(...)`, `Import.Type.{ … }` and
  `Import.Type.Tag` carry the import's nominal exactly as a local one does, so
  `ranked_methods` finds `Counter.get` in the imported module's method table, and a
  method returning a record (B103) or updating one (B098) is typed by its declaring
  module.
- **Type aliases of tag unions keep their methods** (issue 8637): an alias resolves to
  the nominal it names, at any nesting depth (5+ levels), and through a re-export.
- **Generic helpers stay generic.** A `where` constraint on a helper's parameter is a
  constraint, not a binding: instantiate the helper per call site (the checker already
  generalises let-bound functions; the method target has to be part of what is
  instantiated, not resolved once at the definition). The same instantiation gives
  `F64`/`F32` specialisations of one comparison and lets an unconstrained `[]` or
  numeral stay replaceable until the first constraint.
- **`|>` supplies the first explicit argument** of a method call, after the receiver.
- **`Str.inspect` on a function value** whose nominal has `to_inspect` calls it.

Check: `tests/check_eval.sh --filter cross-module --filter "static dispatch" --filter 8637 --filter "block-local" --filter "generic dispatch" --filter where --filter specialization --filter pipe --filter B098 --filter B103 --filter 10049 --filter 11099 --filter 11189`.

### Phase 11: optional and defaulted record fields — PARTLY DONE

*Turned green: 5 (1,786 → 1,791).* What landed:

- **`.?` chains ride the `Ok` path.** `a.?b.c` and `a.?b.?c` desugar to a match that
  maps each following access through the `Ok`, short-circuiting to `Err` on a missing
  slot, so `o.?b.c ?? 0` and `o.?b.?c ?? 11` work whether the slot is present or absent.
- **Both branches of an `if` are checked against the expected type** rather than joined
  and unified, so `make = |cond| if cond { a: 5 } else {}` against `{ a ?: U8 }` type
  checks — joining `{ a: 5 }` with `{}` had failed.

*Follow-up (type-level monomorphization, +9 across phases 10 and 11, 1,791 → 1,800):*

- **A qualified call checks its arguments against the declared signature**, in order,
  instead of synthesising them all first — so `List.map(xs, |r| r.?a ?? 10)` types the
  lambda's parameter as the list's element before its body runs. Mixed-presence lists
  and `??` defaults stop defaulting to `Dec`; this also closed issue 10763 (Phase 10).
- **`{}` and partial records materialize a nominal's defaults** when checked against a
  defaulted nominal — directly, through an `if`, as a `List(Foo)` element, or as a
  `|_| {}` lambda body. The checker records the site (node → nominal) and the compiler
  fills each omitted field with its default expression (or `<missing>` for an optional),
  from `nominal_defaults`, imported modules included — closing issue 11024 and the
  imported/comptime-default tests. A `check`-arm for `match` (bodies checked against the
  expected type, exhaustiveness kept via a shared helper) lets a `|_| {}` lambda reach it.

Still out — **value-level** monomorphization: issue 11271, where `{}` flows through a
generic `id` / `List.repeat` / `List.map` and would materialize at the call-site
specialization; rocflight builds `{}` once as a value and would need to specialize the
callee per call site or carry the type on the value. Also the generalized-constructor
and optional-`Ok`/`Err`-pattern cases, and the closed-param problem tests.

Earlier residue this closed, kept for the record: the **`{}`-materializes-defaults**
cluster (issues 11024 and 11271, a generalized construction, mixed-presence lists) —
`{}` flows through a generic `id`, `List.repeat`, `List.map` or a branch to a
defaulted-record annotation, and the defaults must materialize at the construction
site the way roc's monomorphization arranges; rocflight builds `{}` once and cannot.
Also: an **optional field matched as `Ok`/`Err`** in a record pattern; **imported and
comptime-constant defaults**; and the **closed-param and generalized-update problem
tests** roc refuses (Phase 16). A `??` default over an optional field read through a
generic lambda parameter still defaults to `Dec`, the same qualified-call lambda-typing
gap as Phase 10.

The plan as it was:

*Turns green: 14.* Tests: the four `.?` chain tests (`two-optional chain hit`,
`… short-circuits on the inner missing slot`, `required segment after .? rides the Ok
path`, `… after a missing .? falls back`), `conditional presence executes both
branches`, `list of records with mixed presence`, `annotated list literal with mixed
presence elements`, `generalized constructor adopts optional slot layout`, `nested Ok
pattern in a match exercises both branches`, the three `defaulted record field:
imported …` / `foreign fn-typed default …` tests, `custom from_numeral default
materializes` (after Phase 8), and issues 11024 (three tests: implicit empty records
materialise defaults, locally and imported) and 11271 (two: polymorphic constructions
retain defaults).

What rocflight does today: `o.?b.?c` fails with `Cannot read optional field c on
Ok({ c: 4 })` — the first `.?` yields a `Try`, and the second reads the `Try` instead of
its payload. A list literal with mixed presence inspects the missing slot wrongly, and a
default that is an imported function, a private def of its module, or a nominal is
not materialised when the record is built from `{}`.

The change:

- **`.?` chains ride the `Ok` path.** `a.?b.?c ?? d` is `match a.?b { Ok(b) => b.?c,
  Err(m) => Err(m) } ?? d`, and a required segment after `.?` (`a.?b.c`) maps through
  the `Ok`. One desugaring in the parser; the VM's `GetOptField` does not change.
- **Defaults materialise at construction**, wherever the record is built: `{}` against
  a type with defaults, a nominal `R.{ req: 1 }`, a polymorphic constructor, either
  branch of an `if`. The default expression is evaluated in its declaring module, so a
  private def or an imported function is in scope. The checker's `missing_fields`
  already names the slots; the compiler has to fill defaulted ones with the default,
  not `Missing`.
- **Mixed presence in a list literal** keeps every element on the annotated layout, so
  `[{ a: 1 }, {}]` inspects as roc inspects it.

Check: `tests/check_eval.sh --filter "optional record field" --filter "defaulted record field" --filter 11024 --filter 11271`.

### Phase 12: 128-bit integers, overflow crashes, bit-exact floats — MOSTLY DONE

*Turned green: 21 (1,800 → 1,821).* What landed:

- **`U128` is a real `u128`** (`Value::U128`), so a value above `i128::MAX` prints,
  compares, hashes, converts and parses as its true magnitude rather than a bit
  pattern read back negative. `call_u128` covers `highest`/`lowest`, `from_str`/
  `to_str`, the logical shifts, bitwise ops, comparison and `order_relative_to`, the
  checked/wrapping/saturating/try arithmetic, `to_f32`/`to_f64`, and unsigned ranges;
  `apply_binop`, `values_equal`/`order_values`/hashing, and a `u128_literals` set carry
  it through operators, equality and literals.
- **Overflow crashes at every width**: plain `//` (min-int over -1) and `negate`
  (min-int) carry the width and crash, `I128` plain add/sub/mul crash via a checked-i128
  path in `BinInt` (its overflow cannot be range-checked after the fact), and `U128`
  arithmetic crashes on overflow.
- **Missing width methods**: `div_ceil_try`/`div_floor_try`, `pow_try` on a base of `±1`
  with a negative exponent (exact). Bit shifts, wrapping/saturating arithmetic and
  `negate` keep the receiver's width, so `a.shl_wrap(2).shr_wrap(1)` type-checks.

Still failing (5): the **transcendental bit-exactness** tests (roc's libm versus Rust's,
a separate port), the **overflow-predicate** test (a shared lambda across widths, the
value-level monomorphization gap), and one SIMD test (Phase 15).

The plan as it was:

*Turns green: 22.* Tests: the eight `low_level - U128.*` tests (`bitwise_and high
word`, `bitwise_not basic`, `from_str parses explicit 128-bit integer`, `shl_wrap modulo
127` and `255`, `shr_wrap top bit is logical`, `shr_zf_wrap modulo 128 is identity`,
plus `U64.bitwise ops combine`), `highest_lowest: U128 boundaries`, `comptime eval -
U128 valid max value`, `crash: U128 plain add overflows`, `crash: I128 plain add
overflows`, issue 9812 and regressions B005 and B006 (signed min-int negate and divide
crash), `integer wrapping arithmetic covers every width`, `signed`/`unsigned integer try
arithmetic covers every width`, `overflow predicates return Bool across scalar and
composite widths`, the two `shift operations preserve type` / `shift single bit round
trip` tests, and the three `transcendental exact bits` tests (`F32`, `F64 inverse upper
branch`, `F64 reduction boundaries`).

What rocflight does today: `Value::Int(i128)` holds a `U128` as a bit pattern, so
`U128.highest` inspects as `-1`, arithmetic on it is signed, and `from_str` of a value
above `i128::MAX` is `bad`; `shr_wrap` on a numeral is `Cannot dispatch … on an
unresolved type`, because the width tests call `U128.shl_wrap(1, 127)` on unpinned
numerals inside a list; `I128` plain arithmetic is checked at i128 by the machine and
never crashes; `I64.lowest.negate()` wraps. The float tests want the bits of `sin`,
`exp`, `atan` and friends to match roc's libm exactly.

The change:

- **`U128` as `u128`.** Either a `Value::U128(u128)` variant or a width flag on
  `Value::Int`; the first is smaller. `call_numeric` already keys on the module name,
  so the arithmetic, comparison, `from_str`, `to_str`, inspect, and bitwise arms add
  one case each; `width_fits` stops special-casing U128.
- **Plain arithmetic crashes at every width**, including 128 bits (`BinInt` already
  does it for 8 to 64) and for `negate` and `div` of the minimum signed value.
- **Numerals inside `U128.*` / `I128.*` calls pin to the module's width**, which
  `pin_numerals_to` does for the other widths already.
- **libm bit-exactness.** roc's `std.math` is zig's; Rust's `f64::sin` is the
  platform libm. Where they differ, port the zig routine (it is a few dozen lines per
  function, with a documented reduction) or link against a known implementation.
  Measure first: `--filter transcendental` names the exact inputs.

Check: `tests/check_eval.sh --filter U128 --filter I128 --filter "every width" --filter shift --filter transcendental --filter B005 --filter B006 --filter 9812`.

### Phase 13: `Set` / `Dict` structural keys and custom codecs — PARTLY DONE

*Turned green: 8 (1,821 → 1,829).* What landed:

- **Structural `Dict` keys.** A record, tuple or tag key hashes structurally: the
  runtime dispatch of `to_hash` finds only `Dict.to_hash`/`Set.to_hash`, none of which
  fit the key's shape, so it falls back to the builtin (`has_structural_builtin`), and
  `hash_bytes` now hashes a record by its fields in name order. That closed the four
  `issue 9725` round-trip tests (record, tuple, nested-tuple, tag keys).
- **`Set` dedup across numeric representations.** A whole `Dec` (and a small `U128`)
  now hashes as the integer it equals, which `values_equal` already treats as equal, so
  `s.insert(2)` deduplicates against a `U64` set member even when the `2` defaulted to
  `Dec`. That closed `Set.remove`, `Set.join_map`, `Set.algebra`, and `Set.zero sized
  items`.

Still failing (~11): `Set.from_iter` and `collect`-into-`Set` (roc's `Builtin.roc`
`Set` code expects roc's `Iter` record `{ len_if_known, step }`, not rocflight's list
or lazy `Iter`); `Set.fold`-sums and the custom-key-hash tests (the nominal's element
type is dropped, so the fold's accumulator defaults to `Dec` — the parameterized-nominal
gap); and the JSON codec family (`Json.to_str_try`, custom `parser_for`/`encoder_for`
delegation, NaN classification), which is its own subsystem.

The plan as it was:

*Turns green: 18.* Tests: `Set.algebra and map use set membership` (`Set.map`'s lambda
parameter is untyped because a nominal's type arguments are dropped, so `n % 2`
defaults to `Dec`), the four `issue 9725: … as a Dict key round-trips` (record,
record with nested tuple, tuple, tag union), `Set.fold sums the values`, `Set.insert
retains first representative with coherent custom hash`, `Set.join_map collapses
overlaps in traversal order`, `Set.remove uses last entry and duplicate insert preserves
position`, `Set.zero sized items`, `Set.JSON parsing deduplicates and encoding preserves
iteration order`, `Set.from_iter …` (Phase 9), issue 11063 (custom codec wrapping a
record holding the same codec), the two issue 11094 tests (custom codec delegating to
`Bool`'s encoder / parser through its type parameter), issue 9796 (multiple parser
expects with a forward alias), `JSON encoding classifies every NaN representation
identically`, and `allocation - List.update repeatedly moves unique nested list`.

What rocflight does today: a `Dict` keyed by a record or tuple fails at run time in
hashing; `Set.fold` folds the backing `Dict`'s pairs because `Set :: Dict(item, {})`
drops the type argument on the way through `fold`; `Json.parse` into a `Set` parses a
`List` and the `to_list` comparison sees the duplicate (`No match arm matched`).

The change:

- **Structural hashing and equality for every value shape**: records (by field, in
  declared order), tuples, tag unions with payloads, and zero-sized values (unit, empty
  record) hash and compare in `order_values` / the Dec-and-NaN-canonical hasher. That
  is one `match` arm per shape, where the `Dec` and `NaN` cases already are.
- **`Set` methods go through `Set`**, not the backing `Dict`: `fold`, `join_map`,
  `remove` and `insert` with a custom `to_hash` keep roc's ordering (first
  representative kept; last entry moves on remove; duplicate insert keeps position).
- **Codecs.** `Json.parse` into a nominal calls its `parse` method, and a nominal's
  `encode` / `parse` that delegates to a type parameter's encoder gets the concrete
  one at the call site (Phase 10's instantiation). `Set` gets `parse` (dedup) and
  `encode` (iteration order). NaN encodes identically for every bit pattern.

Check: `tests/check_eval.sh --filter "Dict key" --filter "Set." --filter codec --filter JSON --filter 9796 --filter "List.update"`.

### Phase 14: crypto — DONE

*Turned green: 18 (1,829 → 1,847); all 18 crypto tests pass.* What landed:

- **SHA-256 and BLAKE3 in Rust** (`src/eval/crypto.rs`), by hand so no new dependency —
  SHA-256 the textbook block loop, BLAKE3 the reference chunk-and-tree algorithm. Both
  match the standard `abc` and empty-input vectors, checked by a unit test.
- **The `Crypto.SHA256.*` / `Crypto.BLAKE3.*` API.** The parser collapses the nested
  chains (`Crypto.SHA256.hash`, `Crypto.SHA256.Hasher.empty`, `Crypto.SHA256.Digest.to_hex`)
  into synthetic qualified modules (`Sha256`, `Sha256Hasher`, `Sha256Digest`), the
  checker types them (a `Digest`/`Hasher` nominal), and the evaluator computes them. A
  `Digest` is `Tag("CryptoDigest", [List(U8)])`; a `Hasher` carries its algorithm and
  the bytes written, so `finish` is pure and can be called twice. `hash`, `hash_chunks`,
  `Hasher.empty`/`write`/`finish`, `Digest.to_hex`/`to_bytes`/`is_eq`/`from_bytes`/
  `from_hex` all work in both qualified and method syntax, with `WrongLength` and
  `InvalidHex` errors carrying their `{ expected, actual }` / `{ index, byte }`.

The plan as it was:

*Turns green: 18.* Tests: every name starting `Crypto` in `eval_crypto_tests.zig`:
`SHA256`/`BLAKE3` `hash to_hex`, `hash to_bytes`, `hash_chunks`, `Hasher write finish`,
`finish does not consume prior state`, `from_hex accepts hex`/`uppercase`, `from_hex
rejects invalid hex`/`wrong length`, `from_bytes rejects wrong length`.

`Builtin.roc` declares both (lines ~2883 and ~2965) as `SHA256 :: {}.{ Digest ::
{ bytes : List(U8) }.{ to_bytes, to_hex, from_bytes, from_hex, is_eq, to_hash,
to_inspect }, Hasher :: { state : List(U8) }.{ empty, write, finish }, hash :
List(U8) -> Digest, hash_chunks : Iter(List(U8)) -> Digest }`; BLAKE3 is identical
with a 32-byte digest. roc's oracle is zig's `std.crypto.hash.sha2.Sha256` and
`Blake3`.

The change: SHA-256 and BLAKE3 in Rust, in a new `src/eval/crypto.rs`. Cargo has no
crypto crate today; `sha2` and `blake3` are the standard ones, or SHA-256 is ~80 lines
and BLAKE3 ~150 by hand from the specs — choose by whether a new dependency is
acceptable. `Hasher` is a value holding the bytes written so far (the `state :
List(U8)` in the declaration), so `finish` on it twice gives the same digest; `Digest`
is a nominal over `List(U8)` with `to_hex` lowercase, `from_hex` accepting either case
and refusing other lengths with `Crypto.DigestHexErr`, `from_bytes` refusing other
lengths with `Crypto.DigestBytesErr`. `hash_chunks` consumes a Phase 9 `Iter`.

Check: `tests/check_eval.sh --filter Crypto`.

### Phase 15: SIMD vector types — DONE

*Turned green: 38 (1,847 → 1,885); the 39th is the opt-in differential corpus, which
the runner excludes by default.* What landed:

- **`Value::Simd { kind, bits }`** — a 128-bit vector, `kind` the element width
  (8/16/32/64) with `0x80` for signed, `bits` the lanes packed little-endian. One
  `u128` plus a byte, so `Value` stays small; the eight types register in `module_for`
  and the checker's `simd_result`.
- **The reached methods** (`src/eval/mod.rs::call_simd`): `default`, `splat`,
  `with_lane`, `get_lane`, `broadcast_lane`, `from_list`/`to_list`,
  `from_u128_bits`/`to_u128_bits`, `is_eq` (all 128 bits), `to_inspect`, and
  `concat_shift_bytes`. A lane index or shift count out of range crashes, as roc's
  does. `Str.inspect` renders `U8x16(0, 1, 0, …)`, signed lanes sign-extended. The
  parser already resolves `U8x16.default()` as a qualified call, and a nominal over a
  SIMD backing inspects as the backing type.

Still failing (1): a nominal that defines its own `to_inspect` over a SIMD backing —
the erased `Simd` value carries no nominal tag, so `inspect` cannot find the custom
method.

The plan as it was:

*Turns green: 39.* Tests: `eval_simd_tests.zig` for all eight types (`U8x16`, `I8x16`,
`U16x8`, `I16x8`, `U32x4`, `I32x4`, `U64x2`, `I64x2`): `get_lane` / `with_lane` /
`broadcast_lane rejects index N` (three per type, expected `crash`), `concat shift
rejects counts above sixteen`, `structural equality compares all 128 bits`, and the
issue 11170 lane-inspect tests in `eval_issue_tests.zig`, which build vectors through
nested nominals and inspect lanes.

`Builtin.roc` declares each as `U8x16 :: [ProvidedByCompiler].{ … }` (line ~17468 to
~21000) with ~70 methods. The tests reach: `default`, `splat`, `from_list`, `to_list`,
`with_lane`, `get_lane`, `broadcast_lane`, `min`, `max`, `any`, `all`, `times`,
`from_u128_bits`, `to_u128_bits`, `store`, `load`, `append_to`, `concat_shift_bytes`,
`is_eq`, `to_inspect`.

The change: `Value::Simd { kind: u8, bits: u128 }` — one 128-bit pattern and a kind
code (element width and signedness, as `width_code` already encodes), with lane
arithmetic done by splitting the pattern in `call_numeric`'s style. The reached methods
are each a few lines over the lanes; index and count out of range crash with roc's
message. The eight types register in `module_of` and the checker's `declared()` so a
nominal declared over a SIMD backing (`Pixels :: U8x16`) resolves. Nothing else in the
suite touches them, which is why this phase is last of the builtin phases.

Check: `tests/check_eval.sh --filter SIMD --filter 11170`.

### Phase 16: the checker's remaining refusals — PARTLY DONE

*Refused 2 more (48 → 50 of 71): `problem: to_inspect must return Str` — a `to_inspect`
method whose declared return is not `Str` is a compile problem (`method_problems`); and
`regression B031` — an unpinned fractional literal that overflows what a `Dec` holds
(`999…999.0`) is rejected even before its type is pinned, by defaulting the numeral.
These do not move the backend tally (problem tests run no backend); they raise the
refused-problem count.*

Still accepted (~16): the closed-param and extension-alias strictness (B063/B090/B096/
B097), mutual recursion in untyped locals, polymorphic top-level constants and match
branches (B091, B092, polymorphic-match), and comptime unreachable-branch reporting.
Each is a distinct refusal rule whose risk is over-refusing a valid program, so they
are left rather than guessed.

### Phase 16: the checker's remaining refusals — original notes

*Refuses: 23 problem tests (48 of 71 → 71).* Grouped by the rule each needs:

| Tests | Rule |
|---:|---|
| 2 | `problem: mutual recursion in local lambdas` / `in untyped closures`: two local closures that call each other without annotations are a problem, not a program |
| 3 | `problem: polymorphic erroneous match branch` / `in block`, regression B091: a polymorphic top-level constant, or a branch that is wrong at one instantiation, is rejected before running |
| 4 | `comptime exhaustiveness - unused if branch` / `unused match alternative`, `comptime eval - unused top-level constant crash` / `imported unused top-level crash`: a branch that can never be taken, and a `crash` reachable from a top-level constant, are reported even when nothing runs them |
| 4 | regressions B063, B090, B096, B097: an extensible alias (`Alias(ext) : [A, ..ext]` or `{ a : U8, ..ext }`) rejects a non-union / non-record extension and duplicate tags / fields from it |
| 2 | regression B056, issue 9389: a missing method inside a function body, or on a list, is a problem at check time, not a crash at run time |
| 3 | `optional record field: closed param rejects …` (three): a closed record parameter rejects a wider argument whether the extra slot is present or missing |
| 4 | `problem: to_inspect must return Str`, B031 (a fraction literal with more digits than `Dec` holds), B092 (`List.sum([])` with no element type), B095 (a record-builder `map2` must return `B(c)`) |
| 1 | `custom from_numeral Err in an uncalled function`: a literal conversion runs at compile time, so its `Err` is a problem even in a function nothing calls (Phase 8's residue) |

Each is a `declaration_problems` / `literal_problems` style rule; `rocflight eval`
answers `CompileError` and the runner accepts it. None has a run-time counterpart, so
none can regress a passing backend test except by over-refusing — `tests/check_eval.sh
--report` prints the accepted-problem table, and the backend tally is the guard.

Check: `tests/check_eval.sh --filter problem --filter "comptime exhaustiveness" --filter "unused top-level" --filter B063 --filter B090 --filter B096 --filter B097 --filter B056 --filter 9389 --filter "closed param" --filter B031 --filter B092 --filter B095`.

### Phase 17: the long tail — PARTLY DONE

*Turned green: 5 (1,885 → 1,890).* What landed:

- **`var` reassigned through a tuple pattern** (`(word, index) = get_pair(index)` where
  `index` is a `var`) now reassigns it rather than shadowing, so the `while` loop that
  read it as its counter no longer spins forever (issue 8729).
- **`Str.to_utf8` is typed `List(U8)`**, so `line.to_utf8()` on an unresolved parameter
  no longer comes back as a list of an unknown element and a literal beside it stops
  defaulting to `Dec` (issue 8618).
- **`as` patterns count toward exhaustiveness** — `Ok(n) as whole` covers `Ok`, so a
  `match` of `as`-patterns is no longer reported as missing cases.
- **`while True` is bottom, not `{}`**: an infinite loop constrains nothing, so a
  function whose only exit is a `return` inside it types (no synthetic false return).
- **Long-form `Dec` literal patterns match exactly** — `Pattern::Float` carries the
  exact scaled value, so `1.000000000000000001 =>` matches the `Dec` it was written as
  instead of the nearest `f64`.

Still open (~19): the `match-dt` as-pattern was fixed but the whitespace-postfix pipe,
the `map2` record-builder generic inference, nested `Str.inspect` through a nominal's
`to_inspect`, a zero-sized `with_capacity`, cross-module tag matching, the padding
fields, `trmc` NQueens, and the B028/B059 crash-vs-problem classification remain — each
its own one-off, several gated on the shared mechanisms (parameterized nominals,
value-level monomorphization, nominal erasure).

The plan as it was:

*Turns green: 24*, one bug each, in the order the report lists them:

- `comptime eval - deeply nested with multiple items at each level`; regressions B028
  and B059: a pattern that fails inside a compile-time constant is a Roc crash, not a
  compile problem.
- `interpreter: map2 record builder drops intermediate concat result`, `projecting
  value from owned aggregate drops sibling help`: record-builder desugaring.
- `trmc benchmark: NQueens (n=9)`; `nominal record with unnamed padding reads correct
  fields` and `… refcounted-typed padding is never refcounted`: `_ : T` padding fields
  in nominal records.
- `tag union matching with payload inside function - cross module` and `tag union
  payload matching inside function cross module`: an imported tag with payload matched
  in a function body.
- `Str.iter_utf8 repeated next and for consumption`, `List.concat with Str.to_utf8
  inside lambda (issue 8618)`, `zero-sized list with_capacity reports zero capacity`,
  `nested Str.inspect uses payload to_inspect`, `string interpolation pattern stops at
  first delimiter byte`.
- `match-dt: as-pattern binds whole value alongside payload`, `match-dt: Dec long-form
  literal patterns use wide equality`, `whitespace-separated postfix applies to
  completed pipe`.
- Issues 10763 (separate calls instantiate a partial scheme independently), 11243
  (direct call compared with a bare `Bool` tag compares at the callee's result type),
  11189 (namespaced comparison helper from a nested `fold_with_index`).

Check: `tests/check_eval.sh --strict`.

## The remaining residue: phases 18 to 25

*Measured 2026-09-18 at 1,890 of 1,953, 50 of 71 problem tests refused.* Phases 8 to
17 cleared every self-contained family (numerals, iterators, 128-bit, crypto, SIMD).
What is left — 62 backend tests and ~16 problem tests — does not split by feature; it
splits by a handful of **shared mechanisms rocflight has never had**, each of which
gates a cross-section of the residue. The phases below are those mechanisms, ordered so
the one that unblocks the most comes first. A test is listed under the mechanism it
needs first; several need two.

### Phase 18: nominals carry their identity at run time — DONE (+4, 2026-09-18)

*Turned green: 4 (14/14 on the filter).* The recurring "nominal erasure" wall. rocflight
throws a nominal away once built — a `Color` is a bare tag, a SIMD `Vector` a bare vector
— so `inspect` and dispatch could not find a method the nominal declared. Tests: `inspect:
nested Str.inspect uses payload to_inspect`, `inspect: function-value Str.inspect
preserves nominal method`, `issue 11170: nominal custom inspect takes precedence over
SIMD backing`.

No new wrapper was needed — the existing shape-based dispatch (`NominalShape::is_exactly`
+ `methods_named`) already reconstructs a nominal from a value's shape. Three gaps closed
it:

- **First-class builtins in tail position.** `apply = |f, x| f(x)` with `f = Str.inspect`
  compiled `f(x)` to `Op::TailCall`, whose `func` branch used `as_closure` and rejected a
  `Value::Builtin`. Added a builtin branch to `Op::TailCall` in `src/vm/mod.rs` mirroring
  the one `Op::Call` already had: call `eval::call_function` and return as this frame's
  result.
- **SIMD-backed nominals.** `NominalShape` had no arm for `Value::Simd`, so `Vector :=
  U64x2` never matched its own vector. Added `NominalShape::Simd(u8)` (element-kind byte),
  detected it in `shape_of` via `eval::simd_kind`, and matched it in `is_exactly`/`admits`.
- **Top-level `to_inspect`.** The harness rendered `main` with `eval::inspect` AFTER the
  run returned, when the program was already off the `RUNNING` stack, so `custom_inspect`
  found no methods. `eval::inspect` now consults `custom_inspect` first (correct anyway:
  roc applies `to_inspect` nested too), and `vm::run_and_inspect` renders it inside the
  running scope, threaded through `run::Options.inspect_result` for `rocflight eval`.

Check: `--filter "to_inspect uses payload" --filter 11170 --filter "function-value"`
(use `-- --timeout 5000` — the default 60ms flags slow starts as HANG under load).

### Phase 19: parameterized nominals — type arguments carried — DONE (+3, 2026-09-18)

*Turned green: 3 (filter 5/5; the other two were already green).* `Set(U64)`, `Dict(k, v)`
and a user `Wrapper(a)` dropped their type arguments, so a method reading the element got
an unconstrained variable that defaulted to `Dec`. Tests: `inspect: Set.fold sums the
values`, `issue 10049: structural containers honor component equality and hash methods`,
`inspect: Range.custom iterates a third-party numeric type`.

No new `Type::Nominal` argument list was needed — a nominal's backing already carries its
parameters (`Dict(k, v)` threads `k`/`v` through `entries: List((k, v))`), and unifying
two nominals of one name unifies their backings. Four fixes:

- **`Set` element threading.** `Set(item) :: Dict(item, {})`, but `Set`'s signatures were
  parsed alone, where `Dict` was an unknown placeholder that dropped `item` — so
  `Set.from_list([…U64]).to_list()` came back a list of unconstrained numbers. Now
  `parse_signatures("Set")` parses `Dict`'s declaration first, then keeps only `Set`'s
  own signatures (`src/builtin.rs`).
- **Structural container equality.** A `Dict`/`Set` nested in a record/tuple/tag/list
  compared by its erased insertion-order layout. `eval::values_equal` now dispatches a
  component's own `is_eq` (Dict/Set) when it is a `Tag`/`Record` — order-independent, and
  it terminates because `Dict.is_eq` compares entries, never the whole container.
- **Structural container hashing.** Likewise `eval::hash_bytes` runs a component's own
  `to_hash` against a fresh hasher and uses the resulting state, so a dict nested in a key
  hashes independent of insertion order and bucket layout.
- **`Range` over a third-party type.** `Range(a)` parses to `Nominal{Range, backing: a}`
  and the checker supplies `Range.custom`/`iter`/`size_hint` signatures ahead of the
  `Range` → `List` alias, so a `Range.custom` config pins its numbers to the element. A
  unify arm lets a `Range` nominal's backing match a `List`/`..` element, keeping integer
  ranges iterable. `eval::call_range` builds the config and dispatches the element's
  `range_iter`. Check: `--filter "Set.fold" --filter 10049 --filter "Range.custom"`.

### Phase 20: value-level monomorphization — MOSTLY DONE (+12, 2026-09-19)

*Turned green: 12 (estimate was 8).* Type-level monomorphization landed in phases 10 and
11 (a qualified call checks its lambda against the declared signature); what remained
was the RUNTIME half — a `{}` or a numeral that flows through a generic `id` /
`List.repeat` / `List.map` and must specialize at the call site.

**No per-call-site body specialization was needed for eleven of the twelve.** The
failures were not one mechanism but five independent holes in the checker, each of
which cost a cross-section. All five are in `src/types/checker.rs`:

- **A zero-argument call never peeled its arrow.** `synth`'s `Expr::Call` arm applies
  one arrow per argument, so `f()` — zero arguments, and a zero-parameter lambda's type
  is `{} -> r` — synthesised to the FUNCTION. `force_strings(empty())` unified
  `{} -> List(a)` with `List(Str)`. Peeled explicitly when `args.is_empty()` and the
  callee applies to `Function(Unit, _)`.
- **`instantiate` closed every record it copied.** `substitute_vars`'s `Type::Record`
  arm rebuilt through `Type::closed_record`, dropping `open`. So a generalised
  `get_help = |c| c.help` — whose parameter FieldAccess correctly infers `{ help: a, .. }` —
  demanded a record of exactly `help` at every use, and `get_help(map2(p1, p2, f))`
  failed with "unexpected field `value`". Carrying `open` (and descending into
  `Type::Optional`, which the arm also skipped) fixed the record-builder family.
- **An optional slot rejected a numeral.** `{ a ?: U8 }` accepts `{ a: 5 }`, but the
  `(Optional(inner), other)` unify arm sat BELOW the TypeVar arms, so a numeral variable
  met `U8?` first and the numeral guard refused it outright ("A number cannot be used as
  U8?"). A narrow `(Optional, TypeVar)` arm ahead of the TypeVar cases peels it.
  Its one exception is a record UPDATE — roc rejects `|r| { ..r, a: 5 }` applied to a
  `{ a ?: U64 }` — so `Expr::RecordUpdate` marks the field-value variables of an
  unknown base in `committed_vars`, and the peel declines those.
- **A call's expectation never reached its result.** `check` had no `Expr::Call` arm, so
  `value : Config = id({})` synthesised `id({})` (giving a bare `{}`) and only THEN
  unified with `Config` — too late for the `Expr::Unit` arm that records a
  `default_sites` entry. The new arm unifies the callee's RESULT with the expectation
  first, then checks each argument against its now-resolved parameter; for a qualified
  callee it takes the signature from `declared`, so `List.repeat({}, 2)` and
  `List.map(xs, |_| {})` push `Config` into the unit as well. It repeats the fallback
  arm's `coerce_values` registration — leaving it out cost the three `imported
  polymorphic … lowers its generated operand` tests.
- **`min`/`max` always read as the LIST method.** `builtin_result` answered a `Try` for
  both, so an unresolved receiver made `c.x > a.x.min(b.x)` compare a number with a
  `Result`. The numeric two-argument form is told apart by arity alone.

Also turned green, as a side effect: the three `iterator-like map`/`keep_if`/`drop_if`
tests of Phase 22, `issue 11243`, and `unconstrained empty list specialization`.

**What is left needs the real thing.** Two tests want a value to carry its call site's
WIDTH into a shared body, which rocflight cannot do without tagging integers by width
at run time: `numeric default specialization remains replaceable until constrained`
(`add_one = |x| x + 1` must be `Dec` at one site and `U8` at another — the body's `1`
is one literal) and `overflow predicates return Bool across scalar and composite
widths` (`u8 = |a, b| a.plus_overflows(b)` must overflow at 8 bits at one site and not
at another; every integer value says `I64`). Generalising the lambda's numeral variable
was tried and reverted: it buys neither test and costs six others, because an
annotation's variable ids and the checker's fresh ones share a number space, so
propagating numeral-ness through `instantiate` mismarks `List.append`'s element.

Check: `--filter 11271 --filter "map2 record builder" --filter 11189 --filter
"unconstrained empty list" --filter "projecting value" --filter 11243`.

### Phase 21: block-local and cross-module nominal methods with capture — DONE (+11, 2026-09-19)

*Turned green: 11 (27/27 on the filter; the estimate was 13, and two of the listed tests
were already green).* Two mechanisms, and a parser bug that was masquerading as a third.

**Block-local nominal methods now capture.** The parser hoisted every `.{ … }` member to
the OUTERMOST level, so `make = |offset| { Local := […].{ get = |Local(n)| n + offset } … }`
compiled `Local.get` as a top-level chunk with `offset` out of scope. A member declared
inside a block is now bound WHERE IT STANDS (`Parser::local_methods`, keyed by block
depth so a method body's own block does not drain its siblings), ordered by dependency —
roc's method block is a recursive group and these are sequential bindings, so
`first = second` has to follow `second` (`Parser::order_by_dependency`). Four changes
carry it through:

- `Compiler::bind` sets the member's owner while its body compiles, so a sibling is in
  scope unqualified — the same rule a top-level method block already had.
- `Compiler::dispatch` looks for the method as a LOCAL binding first, by the module the
  checker named; with no module named (a generic parameter), a single in-scope
  `Type.method` is the one meant (`unique_scoped_method`).
- A literal pattern against a nominal whose `from_numeral`/`is_eq` are block-local
  compiles to calls on those closures, since the runtime table cannot see them.
- A block-local method that captures NOTHING is also added to the runtime dispatch
  tables, which is how an imported generic helper — compiled long before the block —
  reaches it. One that does capture cannot be: its chunk needs the closure's values.

`Parser::nominal` now answers with the MOST RECENT declaration of a name, so two blocks
may each declare a `Local` of their own.

**Cross-module names resolve.** `TypeChecker::expose` binds `import Foo exposing [bar]`'s
bare name to the module's `Foo.bar` with its own quantified variables (the compiler
already did this; the checker did not, so `bar(0).baz({})` dispatched on an unresolved
type). `allow_dispatch` now EXTENDS rather than assigns, and each module's own `where`
clauses are registered, so an imported `read : item -> U64 where [item.get : …]` stays
generic. A tag that exactly one declared nominal owns at this arity takes its payload
types from that declaration (`nominal_declaring_tag`), which is what types
`CrateMod.Crate(5)`'s `5` as a `U64`; an imported nominal named in `exposing` is attached
by the app's parser as a PLACEHOLDER, so `declare_types` lets a real declaration beat one
and `placeholder_target` follows a cross-module alias (`ThingAlias : ThingMod.Thing`) one
more hop. `ThingMod.Thing.Make(7)` — module, then nominal, then tag — parses as the tag.

**`looks_like_record` rejected `{ id : 7, balance : 99 }`.** A space before the colon
made the parser read a record literal as a BLOCK, and roc's own tests write it that way.
A block can open with an annotation (`f : … <newline> f = …`), so the two are told apart
by the rebinding on the next line first, then by a comma ending the first field — a
function-type annotation has top-level commas of its own. This alone fixed the two
`nominal record with … padding` tests and `Acct.sample`.

Also landed here: **B098** — a record update's result IS its base's row (returning a
snapshot froze it at the fields seen so far), and checking a literal against an OPEN row
grows it; **`value.encode(format)`** is `format.encode_<kind>(value)`, which is literally
how `Builtin.roc` defines `encode` on every scalar, in both the checker and
`dispatch_builtin`.

Check: `--filter "block-local" --filter "static dispatch" --filter 8637 --filter B098
--filter B103 --filter 11099 --filter 11243 --filter "cross module" --filter "capturing
local method" --filter "imported where helper" --filter "mutually recursive data
structures" --filter "nominal record imported" --filter "local equality captures"
--filter "cross-module polymorphic"`.

### Phase 22: an `Iter` `Builtin.roc` can consume, and user `Iter` types — DONE (+3, 2026-09-19)

*Turned green: 3 (9/9 on the filter; the other three of the five fell out of Phase 20,
once `instantiate` stopped closing the records it copied).* No new `Iter` value was
needed — rocflight's iterator stays the list, range or lazy value it walks. Three fixes:

- **`iterator.len_if_known`.** `Builtin.roc`'s `Set.from_iter` and `Dict.from_iter` READ
  that field off roc's `Iter` record. The VM's `GetField` now answers it from the value's
  size hint (`eval::size_hint_of`) for anything that walks as a list, rather than failing
  with "Cannot access field 'len_if_known'".
- **`iter.collect()` into a `Set`.** roc declares
  `collect : Iter(item) -> output where [output.from_iter : …]` — the EXPECTATION picks
  the implementation. `check` records the nominal a `collect()` node is checked against
  (`collect_targets`) and the compiler calls that type's `from_iter`; the builtin
  `collect` materialized a plain list, and `Set.to_list` then had no nominal to match.
  Whether the nominal really has a `from_iter` is left to the compiler, which has the
  definitions, so a name with none falls back to the builtin exactly as before.
- **`n.to_u64()` in method syntax.** The qualified spelling reached `numeric_result`;
  method syntax only ever reached the shared builtin table, which left the result
  unconstrained — so `$sum + byte.to_u64()` defaulted `$sum` to `Dec` and
  `Str.iter_utf8` summed fixed-point.

Check: `--filter "Set.from_iter" --filter "iterator-like" --filter iter_utf8`.

### Phase 23: the JSON codec protocol — DONE (+6, 2026-09-19)

*Turned green: 6 (8/8 on the filter; the estimate was 5).* The reader was shape-blind:
`type_descriptor` knew only `List` and a nominal's NAME, so a record's fields lost their
types and a nominal lost what it wraps. Five changes:

- **The descriptor carries records and what a nominal wraps.** `type_descriptor` gains a
  `Record` arm (each field with its own descriptor) and a second `Nominal` payload —
  the type the nominal wraps, taken from the one tag of its backing that carries a
  single payload (`Opt(a) := [None, Has(a)]` wraps `a`). `json_read_as` reads an object
  FIELD BY FIELD against those descriptors, which is how a field holding a nominal
  reaches that nominal's `parser_for`.
- **Delegation through a type parameter.** `Elem : a` then `Elem.parser_for(encoding)`
  is how roc names a type argument for static dispatch. `Elem` has no definition, so
  `call_builtin` answers any undefined `parser_for`/`encoder_for` with the DERIVED one —
  `Json.elem_parse` / `Json.elem_encode` — which reads or writes whatever the wrapped
  descriptor says. The descriptor is a thread-local stack (`WRAPPED`), pushed for
  exactly as long as a nominal's own parser runs, because the delegation goes through
  Roc code with nowhere to carry it.
- **`encoding.parse_null`** answers the REMAINING text rather than a value with it —
  there is no value in a `null` — which is what `Opt.parser_for` branches on.
- **`Set`.** `Builtin.roc` declares `Set.parser_for`/`encoder_for` with no body (the
  compiler derives them). A nominal with no `parser_for` but a `from_list` now reads the
  document plain and converts; a value whose shape answers `Set.to_list` encodes as its
  element list, since a `Set` is a `Dict` is a bucket record inside a `HashMap` tag and
  none of that is what it stands for.
- **`Json.to_str_try`** refuses a non-finite number and says which — `Err(NaN)`,
  `Err(Infinity)`, `Err(NegativeInfinity)` — because JSON has no syntax for one.

Two things outside the protocol came with `issue 9796`: **`{ x }` is a record pun at a
block's statement or tail position** and a block whose value is `x` everywhere else
(a binding's right-hand side, a call argument, a list element) — verified against
`roc check`, which accepts `y : { x : U64 } = { { x } }` and rejects `y = { x }`,
`y = f({ x })` and `y = [{ x }]`. And **`List.find_first_index`/`find_last_index`**,
which answer a `Try(U64, [NotFound])`.

Check: `--filter codec --filter 11063 --filter 11094 --filter 9796 --filter "Set.JSON" --filter "NaN representation"`.

### Phase 24: the checker's last refusals — MOSTLY DONE (+14 refused, 2026-09-19)

*51 of 72 problem tests refused → **65 of 72**, with the backend tally unmoved at 1,936.*
(The suite holds 72 problem tests, not the 71 this plan long said: the old count missed
`comptime eval - imported unused top-level crash`, whose entry nests an import's own
`.name`.) Each rule is its own, and the risk throughout is over-refusing a valid
program, so every one was measured against the WHOLE suite rather than its own filter.
Six landed:

- **A closed record parameter refuses a wider argument (3).** `f : { b : Str } -> Str`
  applied to a `{ a ?: U64, b : Str }` was accepted because record unification let the
  left side carry an OPTIONAL field the right lacked. An open row promises only what it
  lists; a closed one lists them all, and roc refuses the wider value because the
  layouts differ.
- **Extension aliases (B063, B090, B096, B097).** `R(x) : { a : I64, ..x }` extends a
  RECORD and `T(x) : [A, ..x]` a tag union; neither may bring a member the base already
  names. The parser now remembers which parameter is the extension
  (`Parser::extension_aliases`) and checks the argument where the alias is APPLIED. Two
  bugs fell out: a tag union's `..x` left `x` to be read as a tag called `x`, and the
  parser's `substitute_type_vars` closed every record it copied, so `R(Ext)` produced a
  closed `{ a : I64 }` that then rejected the extension's own fields.
- **Block-local mutual recursion (2).** roc supports mutual recursion only between
  top-level definitions; a block's bindings run in order. `parse_block_inner` reports a
  binding that names a LATER sibling. It must be shadow-aware — `|value| value == …`
  mentions `value`, but that is the lambda's own parameter — which is what
  `Parser::mentions_free` is for; the naive version cost two backend tests.
- **A method roc does not declare (9389, B056).** `list.reverse()` is `rev` spelled
  wrong and `1.nope()` is nothing at all. `builtin::declared_names()` is every method
  name `Builtin.roc` declares anywhere in it, scanned once; a dispatch on a builtin
  container or a numeric receiver whose method is in neither that set nor the program is
  refused. (rocflight had implemented `reverse` as an alias for `rev`; roc has no such
  method.)
- **A crashing top-level constant (2).** roc folds every top-level constant at compile
  time, so a `crash` in one is reported then — used or not, in the app or in a module it
  imports. A FUNCTION is not folded, the program's own value is what the run is FOR, and
  a bare `_` statement is not a definition; all three stay crashes.
- **The record builder's `map2` (B095).** `map2 : B(a), B(b), (a, b -> c) -> B(a)`
  throws the combined value away. The declared result must mention the combining
  function's own result variable.

**Three were tried and reverted, each measured.** They are not cheap to get right and
each costs more than it buys:

- *Unreachable comptime branches (2).* roc's "unused branch" is a WARNING it emits for
  EVERY constant condition — `x = if True 42 else { crash }` gets it too — and the suite
  expects rocflight to keep RUNNING most of them. Refusing them cost **31** backend
  tests. Only a rule fitted to "the `if` is directly `main`'s value" would separate the
  two, which is fitting the suite rather than matching roc.
- *An unresolved polymorphic top-level value (B091, B092).* roc's message — "This
  top-level value still has an unresolved polymorphic type" — is exactly the rule, but
  rocflight's inference leaves far more unresolved than roc's (every recursive nominal,
  most `Try` payloads): **339** backend tests.
- *Rigid annotation variables (polymorphic-match ×2).* `get_err : [Ok(a), Err(e)] -> e`
  promises every `e`, so an `Ok(_) => ""` arm may not pin it. Making an annotation's
  variables rigid while its body is checked turned one of the two green and cost **4**
  backend tests: a `where`-promised variable must stay flexible (`make : Str -> a where
  [a.from_quote : …]` returns a Str on purpose), and rocflight tracks `where` clauses by
  METHOD NAME, not by which variable they cover. Per-variable `where` tracking is the
  prerequisite.

Left, with `custom from_numeral Err in an uncalled function` (which needs the conversion
run at compile time): **7 problem tests**, none of which moves the backend tally.

### Phase 25: libm bit-exactness and the last one-offs

*Turns green: the rest, to 1,953.* The transcendental tests (`F32`/`F64` `sin`/`cos`/
`tan`/`atan` exact bits — roc's zig `std.math` versus Rust's libm) need the zig routine
ported or a matching implementation. Plus the true one-offs: `zero-sized list
with_capacity reports zero capacity` (needs the element type at run time), `nominal
record with unnamed padding` and `refcounted-typed padding` (`_ : T` padding fields),
`trmc benchmark: NQueens (n=9)`, `whitespace-separated postfix applies to completed
pipe` (a pipe-precedence parser subtlety), `List.update repeatedly moves unique nested
list`, `numeric from constructors create ranges in reverse direction` (reverse ranges
with a fractional `step_by`), and `B028`/`B059` (a lambda pattern that fails is a Roc
crash, but rocflight classifies "no match arm matched" as a compile problem to satisfy
the comptime-exhaustiveness tests — these two need the crash-versus-problem distinction
roc draws by whether the failure is in a called function or a folded constant).

## Order and what it should look like

| After phase | Turns green | Expected tally | Measured |
|---|---:|---:|---:|
| 8 numerals | +35 | 1,771 | **1,769** |
| 9 lazy `Iter` | +22 | 1,791 | **1,782** |
| 10 dispatch | +27 | 1,818 | **1,786** (partial: +4) |
| 11 optional fields | +14 | 1,832 | **1,791** (partial: +5) |
| 12 128-bit | +22 | 1,854 | **1,821** (+21) |
| 13 `Set` / codecs | +18 | 1,872 | **1,829** (partial: +8) |
| 14 crypto | +18 | 1,890 | **1,847** (+18) |
| 15 SIMD | +39 | 1,929 | **1,885** (+38; 39th opt-in) |
| 16 problem tests | 48 → 71 refused | 1,885 | **48 → 50 refused** (partial) |
| 17 long tail | +24 | 1,890 | **+5** (partial) |
| 18 nominal identity | +4 | 1,894 | **1,894** (+4) |
| 19 parameterized nominals | +5 | 1,899 | **1,897** (+3) |
| 20 value-level monomorphization | +8 | 1,907 | **1,909** (+12; 2 left, need runtime widths) |
| 21 nominal methods w/ capture | +13 | 1,920 | **1,927** (+11) |
| 22 `Iter` for `Set`/`Dict` | +5 | 1,925 | **1,930** (+3; 3 more fell out of 20) |
| 23 JSON codec protocol | +5 | 1,930 | **1,936** (+6) |
| 24 last refusals | 50 → 71 refused | 1,930 | **51 → 65 of 72 refused**, 1,936 |
| 25 libm + one-offs | +23 | 1,953 | |

Each test is counted under the first phase it needs, so a phase that lands before its
prerequisite (a `from_numeral` default before Phase 8, `hash_chunks` before Phase 9,
codec delegation before Phase 10) measures under its estimate until the other lands.
The measured tally after each phase goes into this table, replacing the blank, and
into `README.md`'s status row.

The order is by dependency and then by how much of the rest each phase unblocks:
Phases 8 to 11 are checker and compiler work that other families lean on; 12 to 15
are self-contained builtins that nothing else depends on, cheapest first; 16 and 17
are one rule or one bug at a time. Phases 18 to 25 are the residue, organised by the
SHARED MECHANISM each needs rather than by feature — nominal identity (18), carried
type arguments (19), value-level monomorphization (20), capturing nominal methods (21),
a `Builtin.roc`-shaped `Iter` (22) — because each mechanism unblocks a cross-section at
once. 19 should come before 22 and 23, which lean on carried type arguments; 24 (pure
refusals) and 25 (one-offs and libm) can be interleaved anywhere.

Three rules, unchanged from the first plan:

1. **A phase is done when its `--filter` is green**, not when its builtins exist. A
   builtin whose test still fails is a builtin with a bug.
2. **Every phase reruns `tests/check_roc.sh --strict`, `tests/check_examples.sh` and
   `tests/bench.sh`.** A lazy `Iter` (Phase 9) and a wider `Value` (Phases 12 and 15)
   are exactly the changes that can cost the register VM its speed; the bench, run
   against the pre-phase binary (`tests/bench_compare.sh`), says whether they did.
3. **Nothing gets wider than the tests ask.** `Builtin.roc` declares ~70 methods per
   SIMD type and 1,109 intrinsics in all; the eval tests reach a few hundred. Implement
   the reached ones; the rest wait for a test.

## Measuring

`tests/check_eval.sh --report` prints the family and problem tables for the current
binary; `--keep` leaves the per-failure files (source, stderr, exit code) under
`target/eval-fails/` and the runner's log beside them. `--filter X` scopes a run to
tests whose name contains `X`; several `--filter`s union; `-- --timeout 5000` reaches
the runner for a hang.

The runner discards a backend's stderr, so a failing test says `RuntimeError` and no
more. `--keep` routes rocflight through `tests/eval_stand_in.sh`, which keeps the
source and stderr of every failure. To bucket by first error line:

```bash
for f in target/eval-fails/*.txt; do
  awk '/--- stderr/{f=1;next} f' "$f" | grep -m1 -E 'rror|Undefined|ambiguous' || echo NO-MESSAGE
done | sed -E 's/ at [^ ]+\.roc:[0-9]+:[0-9]+//g; s/[0-9]+/N/g' | sort | uniq -c | sort -rn
```

Two dead ends already measured, so nobody walks them twice: loading `Builtin.roc`
members wholesale (923, worse — their bodies shadow working Rust builtins), and testing
through a module's `main` (roc folds it at compile time before any backend runs — the
runner's expression tests are what the backends actually execute).

## The first plan (phases 0 to 7, done)

For the record, what each phase did and what it measured:

| Phase | What | Tally after |
|---|---|---:|
| 0 | per-test measurement in the gate (`tests/check_eval.sh`, the stand-in, `--report`) | (in 1) |
| 1 | integer widths: width by dispatch module, `Value::Int` unchanged; bit counts, wrap/checked/saturated/try arithmetic, `from_str` per width | 1,192 |
| 2 | floats and `Dec`: `F32`, `from_bits`/`to_bits`, exact `Dec` from digits, `Dec` overflow crashes, NaN canonical | 1,321 |
| 3 | `Str`, `List`, `Iter`, `Box`: ~60 builtins the tests reached; `Box` as identity; ranges materialised except under walkers | 1,565 |
| 4 | numerals through nominals and recursive tags: `pin_numerals_to`, `numeric_result`, `nominal_literals` | 1,632 |
| 5 | the checker, both directions: `literal_problems`, `declaration_problems`, exhaustiveness, `missing_fields`, `quoted_literals`; 22 → 49 problem tests refused | 1,668 |
| 6 | the parser: `->` calls, `|>` to a bare tag, string-interpolation and `as` patterns, `|var $x|`, `?` in brace-less bodies, padding fields, hex `U128`, `...` | 1,706 |
| 7 | semantics and dispatch: `BinInt` width overflow crashes, short-circuit `and`/`or`, `var` through a pattern, dispatch ranked by shape and nominal depth, direct operator methods; the two hangs | 1,736 |
