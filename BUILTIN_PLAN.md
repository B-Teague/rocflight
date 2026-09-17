# Builtin parity plan

How rocflight reaches parity with `roc-compiler/src/build/roc/Builtin.roc`, and what
that does and does not buy for `tests/check_examples.sh`.

Everything below was measured against the pinned nightly (`nightly-2026-09-03-62fcb65`)
on 2026-09-16, not estimated.

---

## 1. What Builtin.roc actually is

23,555 lines of **Roc**, not Zig. One nominal, `Builtin :: [].{ … }`, holding eleven
top-level members:

| Member | Line | Member | Line |
|---|---|---|---|
| `Encoding` (incl. `Json`) | 60 | `List(_item)` | 3568 |
| `Str` | 2214 | `Box(item)` | 5244 |
| `Hasher` | 2800 | `Dict(k, v)` | 5557 |
| `Crypto` | 2872 | `Set(item)` | 6231 |
| `Iter(item)` | 3044 | `Num` (incl. every width, `Dec`, SIMD) | 6461 |
| `Stream(item)` | 3462 | | |

Zig holds only the layer underneath: `src/builtins/{list,str,num,hash,dec,sort}.zig`.

**The boundary is visible in the source.** A member with a type annotation and no body
is an intrinsic — `canonicalize/BuiltinLowLevel.zig` rewrites it into a lambda running
one of the 502 `LowLevel` ops. A member with a body is ordinary Roc. `Dict` is a full
open-addressing hash table written in Roc, bottoming out on `list_get_unsafe`,
`hasher_finish` and `combine_unordered_hashes`.

Roughly, per module (annotation-only vs. bodied):

| Module | Total | Roc body | Intrinsic |
|---|---|---|---|
| `List` | 93 | 85 | 8 |
| `Dict` | 36 | 34 | 2 |
| `Set` | 32 | 30 | 2 |
| `Iter` | 22 | 22 | 0 |
| `Encoding.Json` | 108 | 108 | 0 |
| `Hasher` | 16 | 0 | 16 |
| `Box` | 4 | 0 | 4 |
| `Num.Dec` | 115 | 83 | 32 |

## 2. The architecture: vendor it and load it

`src/roc/Builtin.roc` is a **verbatim copy** of
`roc-compiler/src/build/roc/Builtin.roc`, vendored the way `tests/roc/examples/` already
vendors the language's own examples. rocflight parses it at startup and binds its
members, so the builtins rocflight offers ARE the builtins roc defines — not a Rust
re-implementation that drifts.

This mirrors the real compiler exactly:

| roc | rocflight |
|---|---|
| parses `Builtin.roc` as the `builtin` module role | parses the vendored copy at startup |
| `BuiltinLowLevel.zig` rewrites annotation-only defs into lambdas running a `LowLevel` op | binds annotation-only defs to a Rust intrinsic of the same name |
| bodied defs are ordinary Roc, compiled like any module | bodied defs are ordinary Roc, compiled like any module |

The boundary is read off the source, not maintained by hand: **a member with a body is
Roc, a member with only an annotation is an intrinsic Rust must supply.** That is the
whole contract, and it is the same one Zig honours.

Re-sync when the pinned nightly moves:

```bash
cp roc-compiler/src/build/roc/Builtin.roc src/roc/Builtin.roc && tests/check_builtin.sh
```

Two consequences worth stating up front.

- **rocflight must parse 23,555 lines of the fullest Roc there is.** Builtin.roc uses
  every corner of the language, and the parser gaps it exposes are real gaps that any
  serious Roc program can hit. Closing them is not overhead on this plan; it is most of
  the value.
- **Startup cost.** Parsing Builtin.roc on every run is work rocflight does not do
  today. `ponytail: parse it at startup and measure; if it hurts, cache the desugared
  AST the way `.rocflight/cache/` already has a place for.`

## 3. The parse gate

`tests/check_builtin.sh`, alongside `check_roc.sh` and `check_examples.sh`.

Parsing the file whole reports one error and hides the other 23,000 lines, so the gate
slices it on the eleven members of `Builtin :: [].{ … }` — they sit at exactly one tab —
de-indents each into a standalone declaration, and parses them one at a time. Two
numbers come out: how many members parse, and how many also type-check. A member that
parses but fails the checker has cleared the first milestone; the checker gaps behind it
are a later phase.

```
$ tests/check_builtin.sh
  FAIL Encoding     2154 lines  Type error at 0:0
  ok   Hasher         72 lines
  ...
  9 of 11 members parse, 2 also type-check
```

**Status: 12 of 12 parse.** Sixteen parser gaps closed, each verified against the real
compiler first and covered by `tests/builtin_syntax_test.rs`:

| Construct | Blocked | Fix |
|---|---|---|
| `()` as an empty parameter list in a type | every member | `parse_type_atom` yields `Type::Unit`, the same type as `{}` |
| a file that is nothing but declarations | `Builtin.roc` itself | `parse_top_level` yields `{}` when the outermost parse runs out of input |
| `if cond { … }` with no `else` | `Dict`, `Iter`, `List` | the absent branch is `{}`; using its value is a *type* error, never a parse error, which is what makes a bare `if` a statement |
| `for (key, value) in …` | `Dict`, `Set` | the loop variable is a PATTERN; anything but a plain name iterates a fresh name and destructures it in the body |
| `for item in list if predicate(item)` | `List` | a guarded loop is the body wrapped in an else-less `if` |
| `?` in EXPRESSION position | `List` | lifted out under a generated `#tryN` name; the case where `?` covers the statement's whole value is handed back to the statement parser |
| `?` as a loop body's last statement | `List` | an assignment evaluates to `{}` whatever its right-hand side does, so it HAS a continuation |
| `?` propagating out of a loop | `List` | the Err arm is `return Err(e)`: `?` leaves the enclosing FUNCTION, and inside a `for` the block's value is `{}` |
| a bare `..` in a record pattern | `Dict`, `Stream`, `Num` | binds the remainder to the throwaway name, because `rest` is also what tells the checker the record is open |
| `Ok(encoded) = expr` | `Encoding` | the existing tuple/record backtracking, extended to a capitalised start |
| a trailing comma in a tag's payload | `Dict` | allowed in a call already; now in `finish_tag` too |
| a multi-parameter function as a record field, and a parameter list split across lines | `Encoding` | `parse_type_operand` collects the comma list speculatively and rewinds unless an arrow follows |
| `-9223372036854775808` | `Num` | parsed WIDE then narrowed: `I64.lowest`'s magnitude is one past `i64::MAX` |
| a QUALIFIED type name — `List(Dict.DictBucket)` | `(low level)` | an uppercase dotted path parses as a nominal-qualified TAG, which the type parser rejected. Worse, a rejected annotation is only skipped, so **the signature was dropped in silence**. The last segment is the name: nested nominals are registered flat |
| `'"' => …` — a grapheme literal as a pattern | `(low level)` | roc has no character type, so it matches the number it denotes |

**The twelfth member.** Slicing on the members of `Builtin :: [].{ … }` missed a
trailing section: the nominal closes at line 21249, and the 2,305 lines after it are
top-level declarations — `list_get_unsafe`, `hasher_finish`, `dict_seed` — the ops the
real compiler injects and Builtin.roc calls but never defines inside the nominal. They
are now their own slice, `(low level)`, and they are **153 of the 226 intrinsics**. A
plan that counted only the members would have under-counted the Rust work by two
thirds.

**The last two were not the parser.** `Iter` needs `18446744073709551615`
(`U64.highest`) and `Num` needs `340282366920938463463374607431768211455`
(`U128.highest`), and rocflight's integer was an `i64`. P6 widened it, and both parse.

## 4. The conformance corpus

Parsing proves rocflight can read Builtin.roc. It does not prove the builtins behave.
The doc comments do that: 1,142 ` ```roc ` blocks holding **1,926 `expect`s**, written
by the Roc authors against the real semantics.

`tests/harvest_builtin.sh` walks the vendored file, tracks the nominal nesting by
indent, and writes one file per fenced block to `tests/roc/builtin/<Module>/<nnn>.roc`.
The output is committed and regenerated when the nightly moves. Each file is then an
ordinary golden input: `roc test <f>` against `rocflight --test <f>`, which the
`--test` split already makes comparable.

One file per block, not per `expect`. Splitting per `expect` breaks 18 of them:
`expect Dict.empty()` is the opening line of a multi-line chain, and
`expect dictionary.get(1) == Ok("Apple")` needs a `dictionary` its snippet defined three
lines earlier.

Measured baseline, for the four modules this plan touches:

| Module | rocflight | Corpus | under real `roc` |
|---|---|---|---|
| `Str` | **13** | 28 blocks / 57 expects | 28/28 |
| `List` | **13** | 63 blocks / 104 expects | 63/63 |
| `Dict` | **0** | 25 blocks / 24 expects | 25/25 |
| `Set` | **0** | 9 blocks / 8 expects | 9/9 |
| | **26** | **125 blocks** | **125/125** |

All 125 pass under the real `roc`. The corpus is clean; every failure is rocflight's.

## 5. Phases

### P0 — vendor and parse *(done: 12 of 12)*
`src/roc/Builtin.roc`, `tests/check_builtin.sh`, and the parser gaps in §3. Every gap
that is a PARSER gap is closed. The last two members are blocked on integer width, so
P0 is done as far as parsing goes and finishes when P6 widens `Value`'s integer.

### P1 — the intrinsic layer *(done)*
`src/builtin.rs` vendors the source with `include_str!`, slices it into members, parses
each, and reports where the builtin boundary falls. `rocflight --builtins` prints it and
`--builtins=names` lists every intrinsic; `tests/check_builtin.sh` is the gate around it.

The split needs no table of its own, because the parser already computes it: an
annotation a binding claims is a definition, and **an annotation left unclaimed when its
block closes is an intrinsic.** That is the same rule `BuiltinLowLevel.zig` applies.

```
$ rocflight --builtins
  ok   Str           586 lines    18 defined    27 intrinsic
  ok   Dict          674 lines    34 defined     2 intrinsic
  ok   (low level)  2305 lines   164 defined   153 intrinsic
  ...
  10 of 12 members parse: 593 definitions in Roc, 226 intrinsics for Rust
```

**593 definitions come for free; 226 intrinsics are the Rust to-do.** Generated, not
guessed, and it re-derives itself when the vendored file is re-synced.

Two findings worth carrying into P4:

- **`Dict` and `Set` declare two intrinsics each** — `parser_for` and `encoder_for`, the
  codec hooks. Everything else about them, the open-addressing table included, is Roc.
  Implementing `Dict` is therefore not writing a hash table; it is supplying the `List`
  and `Hasher` ops underneath it.
- **`Hasher` is 16 intrinsics and 0 definitions.** It is pure low-level surface, so the
  "checker whitelist, not an implementation" shortcut in P4 only holds while nothing
  calls it for real.

### P2 — load it *(done; members load on demand)*
`rocflight::builtin::load` parses the named members and `main.rs` compiles them into the
program ahead of the app, the way `compile_unit` already places a local module's top
level ahead of the app's. A member's method blocks have already qualified its names —
the AST binds `Str.is_empty`, not `is_empty` — so they arrive as ordinary top-level
functions and dispatch finds them like any `Type.method`.

Three things came with it:

- **The bare low-level names resolve.** `list_get_unsafe(xs, i)` has no module, so the
  compiler asks `eval::low_level_arity` and emits a builtin call under a module of its
  own. That function IS the registry, so the list and the implementations cannot drift.
  Ten ops are written: the `list_*_unsafe` family, `hasher_finish`, `dict_pseudo_seed`.
- **Failure is loud and correctly timed.** A name nothing declares is a compile error
  (`Undefined variable: crypto_digest_to_hex`). A name Builtin.roc DECLARES but Rust has
  not written is a runtime message (`low-level op \`u8_from_str\` is not implemented`) —
  the same way a missing `Str.repeat` already behaves, and it lets a member load and run
  everything that does not touch the gap.
- **`--load-builtins=Dict` selects members** for work on them, and the default list is
  empty.

**Why the default is empty, and what it costs.** Every member that defines anything
regresses the gates:

| Loaded | Golden pairs | Examples |
|---|---|---|
| *(nothing)* | 98 | 12 |
| `Hasher` | 98 | 12 |
| `Stream` | 96 | **10** |
| `Box` | 95 | 12 |

`Hasher` is free only because it is 16 annotation-only members and defines nothing.
The cause is in `vm::compile`'s `dispatch`: it looks up a top-level `Type.method` by
**name**, and on a single candidate emits `CallFn` to it whatever the receiver is. So
loading `Stream` makes `[1, 2, 3].map(f)` call `Stream.map`, which matches a Stream
record and fails. On more than one candidate it refuses outright — loading `Dict` and
`Set` together is "``to_hash`` is ambiguous: Dict.to_hash, Set.to_hash".

That rule is fine for a handful of user nominals. It cannot survive Builtin.roc, where
every type defines `map`, `len`, `is_eq`, `to_hash` and `to_inspect`.

**So P3 came before P2 paid off, not after it.** The ordering in this plan was wrong,
and the table above was the evidence. With P3 done, `Stream` loads with no regression at
all — the table's second row is now 98 and 12.

The default is still empty, but for a different and smaller reason: loading a member
costs about a millisecond and 600kB on every run, and `Stream` cannot do anything useful
until `Iter` parses. `load` short-circuits an empty selection so that `SOURCE`, 700kB of
the binary, is never even scanned — without that, merely *offering* the vendored module
cost every program 700kB of resident memory.

### P3 — dispatch by type *(done)*
Resolving a name by what it is applied to, rather than by how it is spelled.

**What it took.** The checker already resolves every receiver; it just never told anyone.
`TypeChecker::dispatch_modules` now reports, per `Dispatch` node, which module the
receiver belongs to — through the finished substitution, exactly as `integer_binops`
already did — and `binop_modules` does the same for operators, because `a == b` is
`a.is_eq(b)` and needs the same answer. `vm::compile` uses them to pick `Type.method`
instead of guessing, and `Program` carries a `(module, method)` table so a dispatch the
checker could NOT type is resolved by the running value instead of at compile time.

**Three bugs it exposed, all latent before Builtin.roc made them visible:**

- `dispatch` committed to the only top-level method with a matching name whatever the
  receiver was. Loading `Stream` made `[1, 2, 3].map(f)` call `Stream.map`.
- `tops.operator_methods` is a program-wide switch: one `is_eq` anywhere sent EVERY
  `==` through a method search, so a lone `Try.is_eq` answered for tuples and tags. It
  is now gated on the operand's type naming a module that defines the method.
- Runtime dispatch fell through to another type's method when a value's own module had
  none — a `List` reaching `Stream.map` again, by the back door.

**Result: `Stream` is the first member that loads at zero cost** — 98 golden pairs and
12 examples either way — and its code demonstrably runs. `Stream.from_iter` goes from
"Unknown function" to executing roc's own source and reaching `Iter.size_hint`.

**Still open, and it is what blocks `Box`.** Roc erases nominals, so an unannotated
`Money.{ cents: 5 }` is a plain record to the checker and a plain record at run time.
Neither end can tell it from any other record, so a method on a record or a tag is still
resolved by name alone. A tuple can be ruled out — it can never be a nominal's backing,
which is what fixes the tuple comparisons — but a tag cannot, so `Box`'s `Try.is_eq`
still captures `Green == Green`. Carrying nominal identity into the AST is the fix, and
`3934434` already put node identity there to build on.

### P3b — the type table *(done)*
`Builtin.roc`'s annotations are now the checker's source of truth for the types it had
no rule for, and the two holes that made "strongly typed" a claim rather than a fact are
closed.

**A type roc names is a type here.** `parse_type_atom` answered a FRESH VARIABLE for any
name it did not recognise, so `fruit_dict : Dict(Str, U64)` had no type at all — the
name was thrown away — and `fruit_dict.get(k)` reported "Cannot dispatch `get` on an
unresolved type". `Dict`, `Set`, `Iter` and `Hasher` are now real nominals, which is what
gives dispatch something to resolve against.

**The signatures come from the source roc compiles.** `TypeChecker::declared` consults
`builtin::signatures_for`, so `Dict.get` is `Try(v, [KeyNotFound])` because that is what
`Builtin.roc:5557` says, not because a `match` on the method name guessed it. The
declared parameters are unified against the arguments, so `Dict.with_capacity("nope")`
is rejected against its `U64` — reading a result type off the end is not type checking.

Two corrections fell out. `Dict.empty()` is a call with no arguments to a `() -> Dict`:
Roc spells an empty parameter list `()`, which IS the unit type, so applying it peels one
arrow — peeling none handed back the function itself, and `Dict.empty().insert(k, v)`
had nothing to dispatch on. And inside `Builtin.roc` a reference to `Str` resolves
through that file's own `Str :: [ProvidedByCompiler]`, so a builtin name now keeps its
builtin meaning even where the file declares it.

**Result: `BasicDict` type-checks.** It now fails at run time with "Unknown function
`Dict.empty`", which is the honest next gap rather than a type error, and loading
`Dict` reaches `U32.shl_wrap` — the numeric width work in P6.

**Two things it deliberately does not do.**

- **A nominal's type ARGUMENTS are dropped.** `Dict(Str, U64)` and `Dict(I64, Bool)` are
  one type, so an element's type is still a variable and `Set(U64).insert("a")` is
  accepted. `Type` needs a parameterised nominal; `tests/builtin_syntax_test.rs` records
  the gap with a test to flip when it lands.
- **The table covers `Dict` and `Set` only.** All ten parsing members are verified to
  seed cleanly — 98 golden pairs and 12 examples with any combination — but `Str` and
  `List` moved no gate and cost real time: `Str` took the `strings` benchmark up 40%,
  and `List` is 1,676 lines and cost 5ms of every run against a 3ms baseline.
  `builtin_result` already gives their common methods real types, so the parse buys a
  long tail nothing yet asks for. Signatures are read lazily per module and cached, so
  a program that never mentions a `Dict` never pays for one; widening `TYPED_MEMBERS` is
  one edit.

### P3c — nominal identity *(done)*

roc erases nominals, so a value cannot say which one it is. Three things closed the gap,
and between them they took `check_examples.sh` to **14/5**.

**A nominal's SHAPE rules one out, which is enough.** `Program::nominal_shapes` records
what each nominal's backing looks like — its tags, its fields, a tuple's arity — and
runtime method lookup drops any candidate whose shape cannot admit the receiver. A tuple
is not a `Try`, so `Try.is_eq` no longer answers `(1, "x") == (1, "x")`. This is the
runtime half; the compiler still resolves from the type wherever the checker knows one,
and only the cases it cannot type reach here. `same = |a, b| a == b` is exactly such a
case: polymorphic, so there is no type to resolve from.

**A sibling method is in scope unqualified.** Inside `Graph :: … .{ … }` roc lets
`from_list` call `from_dict(…)`, but rocflight binds it as `Graph.from_dict`. The bare
name was an undefined variable at run time and — worse — a fresh type variable in the
checker, so every caller of the method it belonged to lost its type. Both the checker
and the compiler now look for `Type.name` when a bare name is unknown and a method block
encloses it.

**A test runs after the whole file is in scope.** `parse_top_level` deferred the expects
it met at the outermost level, but not those inside a `_ = <chain>` — the shape this
parser produces for a file whose declarations are followed by expects. `compile_unit`
reorders them when it flattens, so the VM was already running them last; the CHECKER
walked the tree as written and saw `graph.dfs(…)` before `graph` was bound.
`lift_expects` now moves them in the AST, so both agree.

**`Box` and `Stream` load cleanly now** — 98 golden pairs and 14 examples with them on —
and `builtin::needed_by` brings them in for a program that mentions `Try`, `Box` or
`Stream`. A program that mentions none still starts in 0.8ms.

**`GraphTraversal` passes all 8 tests**, which needed all three of the above plus
`List.rev` — roc's name for what rocflight had called `reverse`.

#### What is left after P3c

- **A nominal's type ARGUMENTS are dropped.** `Dict(Str, U64)` and `Dict(I64, Bool)` are
  one type, so an element's type is still a variable. `Type` needs a parameterised
  nominal; `tests/builtin_syntax_test.rs` records the gap with a test to flip.
- **Two shapes can be identical.** `Set(item) :: Dict(item, {})`, so a Set and a Dict are
  both `HashMap(…)` at run time and only the checker can tell them apart. Nothing
  depends on it yet because the receivers in play are typed.

### P4 — `Dict` and `Set` *(done: **BasicDict** and **GraphTraversal**)*

**`BasicDict` passes all 7 tests**, and what runs is `Builtin.roc`'s own open-addressing
table — bucket array, fingerprints, seed and all — not a Rust `HashMap` behind a
Roc-shaped facade. `Dict.empty().insert("a", 1)` yields
`HashMap({ entries: [("a", 1)], buckets: [{ dist_and_fingerprint: 467, … }], … })`.

What Rust had to supply, all of it named by `Builtin.roc` rather than guessed:

- **The numeric width family.** `shl_wrap`, `shr_wrap`, `shr_zf_wrap`, `bitwise_*`, the
  `to_uN_wrap`/`to_iN` conversions, the division variants, and `highest`/`lowest`. The
  width comes from the MODULE, so `U32.shl_wrap(1, 8)` is 256 and `U8.shl_wrap(200, 1)`
  is 144; a conversion reads its target width off the NAME instead.
- **The `Hasher` family**, 16 intrinsics plus `to_hash` per type, over FNV-1a. Roc
  declares the algorithm to be the compiler's business and requires only that `==`
  implies the same hash.
- **`List.repeat`**, and the capacity trio (`reserve`, `with_capacity`,
  `release_excess_capacity`) which are allocation hints and not observable.

Three real parity bugs surfaced on the way, all of them rocflight's rather than
Builtin.roc's:

- **A bare `True` was not a boolean.** `Bool : [True, False]`, and roc runs `x = True`
  then `if x { … }`; rocflight refused it with "Cannot unify [True, ..] with Bool". Only
  the qualified `Bool.True` had been special-cased.
- **A qualified tag needed a locally declared nominal.** `Try.Ok(x)` is `Ok(x)`, but
  `Try` is declared in `Builtin.roc`, not in the user's file, so every tag the module
  qualifies was an "Unknown function".
- **A nominal backed by another nominal would not unify with it.** `Graph(a) ::
  Dict(a, List(a))` is a Dict wearing a name and roc lets the backing through.

**`U64.highest` saturates to `i64::MAX`.** Wrapping it to `-1` made Builtin.roc's own
guard `if b > U64.highest - a` fire on every insert and crash with "Dict capacity
overflow". rocflight's integers ARE i64, so the representable ceiling is the true one;
reporting it is honest where reporting `-1` is not. A wider `Value` integer removes the
ceiling — see P6.

**Loading follows the source.** Parsing `Dict`, `Set` and the 2,305-line low-level
section costs 17ms against a 3ms baseline, so `builtin::needed_by` reads the member list
off the program: the only way to make a Dict is to name one, so a program that does not
mention `Dict` or `Set` pays nothing and still cannot be wrong. Startup for everything
else stays at 0.9ms.

**GraphTraversal followed in P3c**, once nominal identity landed.

### P5 — `Encoding.Json` *(done: **Json** and **EncodeDecode**)*

`Builtin.roc` derives a JSON codec from the SHAPE being read or written, through the
`Encoding` protocol at `Builtin.roc:60` — 108 members of it. rocflight reads and writes
JSON in Rust instead, and lets a type override it the way roc does:

- **A type's own `encoder_for` wins.** `EncodeDecode`'s `ItemKind` encodes its ten tags
  as the numbers 1 to 10 rather than as their names, and that is its own Roc code
  running: found by shape, handed an encoding, driven over a state.
  `Encoding.encode_u32(n, state)` is the Rust side of the protocol it calls back into.
- **Reading needs the TARGET type**, because `[1,2,3]` is a `List(ItemKind)` only
  because an annotation says so. Nothing at run time can recover that, so the checker
  records what each `Json.parse` call was checked against and the compiler passes it as
  a descriptor. A `Nominal` in it hands the reading to that type's `parser_for`.
- **Without a target it reads the document as it stands** — an object is a record, an
  array a list. Roc records are structural and rocflight erases nominals, so `Json`'s
  `record.image.title` lands on a plain field and no shape is consulted.

`ponytail: the protocol's own 108 members are not implemented — the encode/parse steps
are Rust and the encoding handed to a type is a marker rather than roc's
\`JsonEncoding\`. A format other than JSON would need the real thing.`

### P6 — `Dec` and wide integers *(done: **SafeMath**, and P0's last two members)*

**`Value::Int` is an `i128`.** Roc has `U64`, `I128` and a `Dec`; `Builtin.roc` writes
`U64.highest` and `U128.highest` out in full, and an `i64` could not hold either — which
is what stopped `Iter` and `Num` parsing. `Value` went from 32 bytes to 48 because an
`i128` aligns to 16, measured across the whole benchmark suite before it was accepted:
nothing moved, and `records` got faster.

**`Dec` is a real fixed-point type**, `i128` scaled by 10^18 as
`roc-compiler/src/builtins/dec.zig:141` sets it. Two things it needed:

- **Multiplication without a 256-bit intermediate.** `a * b / SCALE` overflows for
  anything past about 13 — the operands carry 10^18 each, so `25.0 * 25.0` is 6.25e38
  against an i128 ceiling of 1.7e38. Splitting each operand into whole and fractional
  parts keeps every term in range.
- **The literal as WRITTEN, not the nearest double to it.** `Expr::Float` carries both
  the f64 and the same digits scaled, because `147.666666666666666666` is exact as a
  `Dec` and is not as an f64 — which is the whole reason the type exists.

## 6. The compiler work that came with it

Three of the examples were never about `Builtin.roc`. They are here because they are
what feature parity actually cost.

**A numeral has no type until something gives it one.** roc DEFAULTS an unconstrained
one to `Dec` — `x = 15` prints `15.0` and `[1, 2, 3]` prints `[1.0, 2.0, 3.0]`, while
`[1,2,3].len()` is still `3`. rocflight synthesised `I64` for every integer literal, so
it disagreed with the compiler about its own examples. A literal now synthesises a
variable CONSTRAINED to be numeric, and `fractional_literals` reports the ones nothing
ever pinned.

That change alone took 52 of the 98 golden pairs with it, and putting them back is most
of what this phase was:

- A qualified call NAMES its receiver's type (`I64.to_str(birds)`), and a constructor
  RETURNS it (`I64.from_str(s)` is a `Try(I64, …)`).
- A numeral's variable is not quantified, and the constraint travels when unification
  binds it to another — `birds = 3` has ONE type, and generalising gave every use a
  fresh copy that nothing could reach.
- `check` rather than `synth` wherever a type is already known: a call's arguments, a
  list's elements, a record's fields, a tag's payload, a dispatch's arguments. A lambda
  needs its parameter types BEFORE its body is looked at, which is what `xs.fold(0, |a,
  x| …)` nested inside another fold depends on.
- `Substitution::apply` recursed into `List` and `Function` only, so a variable inside a
  tag's payload, a record's field or a tuple was never substituted — anything learned
  about it was learned and then thrown away.
- An unannotated parameter is at least a record WITH the fields read of it, and an open
  record GROWS as more are read.
- A `return` leaves the FUNCTION, so what it hands back is the function's result.

**Record builder syntax.** `{ a: pa, b: pb }.Combiner` folds the fields through the
named type's `map2`. `DateParser.roc` writes the expansion out by hand in its second
test, which is what the desugaring matches.

**Opaque nominals.** `Name :: backing` is the opaque form, and roc shows one as
`<opaque>`. Decided from the SHAPE, because nominals are erased — `ponytail: a record
with exactly an opaque nominal's fields reads as opaque too; carrying the nominal on the
value is what closes that, and every other operation would then have to see through
it.`

## 7. Where it landed

| Gate | Result |
|---|---|
| `tests/check_roc.sh --strict` | **98 / 98** golden pairs |
| `tests/check_examples.sh` | **19 / 19** runnable examples (9 skipped: `roc` itself cannot run them here) |
| `tests/check_builtin.sh --strict` | **12 / 12** members parse |
| `cargo test` | 32 suites |
| `tests/bench.sh` | at baseline, bar the signature parse below |

`Builtin.roc` reports **1,443 definitions in Roc and 1,109 intrinsics for Rust**, and
`Dict`, `Set` and the low-level section are loaded on demand — a program that never
names a `Dict` starts in 0.9ms and parses none of it.

The one standing cost is the signature table: `Str` and `List` are read for their real
types, which a program using either pays about a millisecond for. The bodies are
stripped before parsing, which took `list_ops` from 7ms back to 3ms; the rest is the
annotations themselves.

## 8. The standing rule

Builtin.roc is the **definition**, not a reference to copy from. Where rocflight needs
Rust, it is because the member is annotation-only and roc uses Zig there too — and the
Rust is named after the member it implements. Anything with a Roc body stays Roc. The
1,926 harvested `expect`s decide whether that is actually what happened.
