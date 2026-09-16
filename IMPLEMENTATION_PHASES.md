# Implementation Phases

**Target:** run `roc-compiler/test/echo/all_syntax_test.roc`
**Protocol:** [PHASE_IMPLEMENTATION_GUIDE.md](PHASE_IMPLEMENTATION_GUIDE.md) — read
the golden-pair rule before adding anything
**Verified against:** `roc` nightly-2026-09-03-62fcb65

Every syntax feature gets its own golden pair of `.roc` files: both compile cleanly
under `roc check`, the desugared one carries explicit types, and **`roc` and the
interpreter must produce identical output on both files** — four runs, one answer.
`tests/check_roc.sh --strict` is the gate.

---

## Status

A feature is done when **all four outputs are byte-identical**:

```
        roc run <sugared>   ═══   roc run <desugared>
              ║                          ║
     rocflight <sugared>   ═══   rocflight <desugared>
```

`pair` = both files pass `roc check` and the two `roc run` outputs agree.
`runs` = `rocflight` matches `roc run` on **both** the sugared and the desugared
file. Matching on only one of them is not a pass — that asymmetry is a real bug, and
it has already caught one (top-level bindings could not parse binary operators, which
only the desugared form exposed).

Measured by `tests/check_roc.sh --strict`, not asserted. Regenerate with the loop at
the bottom of this file.

### Done

| # | Feature | Pair | Runs | File |
|---|---------|:----:|:----:|------|
| 01 | String literal | ✅ | ✅ | `01_strings/literal` |
| 01 | String interpolation `${}` | ✅ | ✅ | `01_strings/interpolation` |
| 01 | Escape sequences | ✅ | ✅ | `01_strings/escapes` |
| 01 | Nested string in `${}` | ✅ | ✅ | `01_strings/nested_interpolation` |
| 02 | Decimal int literal | ✅ | ✅ | `02_numbers/int_literal` |
| 02 | Fractional literal | ✅ | ✅ | `02_numbers/frac_literal` |
| 02 | Hex / octal / binary | ✅ | ✅ | `02_numbers/radix_literal` |
| 02 | Type-suffixed literal `255.U8` | ✅ | ✅ | `02_numbers/typed_literal` |
| 04 | `+` `-` `*` `/` | ✅ | ✅ | `04_operators/arithmetic` |
| 04 | Operator precedence | ✅ | ✅ | `04_operators/precedence` |
| 03 | Lambda `\|x\| body` | ✅ | ✅ | `03_lambdas/lambda` |
| 03 | Closure capture | ✅ | ✅ | `03_lambdas/closure` |
| 04 | `//` and `%` | ✅ | ✅ | `04_operators/int_div_mod` |
| 04 | Comparison operators | ✅ | ✅ | `04_operators/comparison` |
| 04 | `and` / `or` / `!` | ✅ | ✅ | `04_operators/boolean` |
| 05 | Headerless platformless app | ✅ | ✅ | `05_entry_point/implicit_header` |
| 05 | Explicit `app [main!] {}` | ✅ | ✅ | `05_entry_point/explicit_header` |
| 05 | `echo!` effect call | ✅ | ✅ | `05_entry_point/echo_effect` |
| 06 | One-line `if` / `else` | ✅ | ✅ | `06_if_else/one_line` |
| 06 | Braced `if` branches | ✅ | ✅ | `06_if_else/braced` |
| 06 | `else if` chain | ✅ | ✅ | `06_if_else/else_if_chain` |
| 06 | Nested / grouped `if` | ✅ | ✅ | `06_if_else/nested_grouped` |
| 09 | Record literal | ✅ | ✅ | `09_records/literal` |
| 09 | Record field access | ✅ | ✅ | `09_records/field_access` |
| 09 | Record from a lambda | ✅ | ✅ | `09_records/returned_from_lambda` |
| 09 | `Str.inspect` field sorting | ✅ | ✅ | `09_records/inspect_sorts_fields` |
| 12 | Bare tags | ✅ | ✅ | `12_tag_unions/bare_tag` |
| 12 | Tag payloads | ✅ | ✅ | `12_tag_unions/payload_tag` |
| 12 | Tag equality | ✅ | ✅ | `12_tag_unions/tag_equality` |
| 12 | Inferred union from branches | ✅ | ✅ | `12_tag_unions/inferred_union` |
| 12 | Nested tag payloads | ✅ | ✅ | `12_tag_unions/nested_payloads` |
| 11 | Tag patterns | ✅ | ✅ | `11_match/tag_patterns` |
| 11 | Literal + wildcard patterns | ✅ | ✅ | `11_match/literal_patterns` |
| 11 | Payload binding | ✅ | ✅ | `11_match/payload_binding` |
| 11 | Alternatives `A \| B` | ✅ | ✅ | `11_match/alternatives` |
| 11 | Guards | ✅ | ✅ | `11_match/guards` |
| 11 | Nested patterns | ✅ | ✅ | `11_match/nested_patterns` |
| 11 | Exact-length list patterns | ✅ | ✅ | `11_match/list_patterns` |
| 11 | List rest patterns `..` | ✅ | ✅ | `11_match/list_rest_patterns` |
| 17 | `?` error propagation | ✅ | ✅ | `17_error_handling/question_operator` |
| 17 | `?` short-circuits | ✅ | ✅ | `17_error_handling/question_short_circuits` |
| 17 | `??` default value | ✅ | ✅ | `17_error_handling/default_operator` |
| 17 | `??` precedence | ✅ | ✅ | `17_error_handling/default_precedence` |
| 15 | List literals | ✅ | ✅ | `15_lists/literal` |
| 15 | `List.len` | ✅ | ✅ | `15_lists/len` |
| 15 | `List.map` | ✅ | ✅ | `15_lists/map` |
| 15 | `List.fold` | ✅ | ✅ | `15_lists/fold` |
| 15 | List equality and nesting | ✅ | ✅ | `15_lists/equality_and_nesting` |
| 10 | Tuple literals | ✅ | ✅ | `10_tuples/literal` |
| 10 | Tuple indexing `.0` | ✅ | ✅ | `10_tuples/index` |
| 10 | Destructuring in a block | ✅ | ✅ | `10_tuples/destructure` |
| 10 | Destructuring at top level | ✅ | ✅ | `10_tuples/destructure_top_level` |
| 10 | Tuple patterns | ✅ | ✅ | `10_tuples/patterns` |
| 10 | Tuple equality | ✅ | ✅ | `10_tuples/equality` |
| 14 | Nominal over a record | ✅ | ✅ | `14_nominal/record_backed` |
| 14 | Nominal over a tag union | ✅ | ✅ | `14_nominal/union_backed` |
| 14 | Nominal destructuring pattern | ✅ | ✅ | `14_nominal/destructure_pattern` |
| 20 | Numeric dispatch `n.to_str()` | ✅ | ✅ | `20_dispatch/numeric` |
| 20 | List dispatch | ✅ | ✅ | `20_dispatch/list_methods` |
| 20 | Str dispatch | ✅ | ✅ | `20_dispatch/str_and_bool` |
| 20 | Chained dispatch | ✅ | ✅ | `20_dispatch/chaining` |
| 13 | Pipeline `\|>` | ✅ | ✅ | `13_pipelines/simple` |
| 13 | Pipeline with arguments | ✅ | ✅ | `13_pipelines/with_args` |
| 13 | Chained pipelines | ✅ | ✅ | `13_pipelines/chained` |
| 13 | Pipeline precedence | ✅ | ✅ | `13_pipelines/precedence` |
| 07 | Negate a variable | ✅ | ✅ | `07_unary_minus/negate_variable` |
| 07 | Negate an expression | ✅ | ✅ | `07_unary_minus/negate_expression` |
| 07 | Unary minus with operators | ✅ | ✅ | `07_unary_minus/with_operators` |
| 16 | `for` loop with `var` | ✅ | ✅ | `16_loops/for_loop` |
| 16 | `while` loop | ✅ | ✅ | `16_loops/while_loop` |
| 16 | `break` | ✅ | ✅ | `16_loops/break_early` |
| 16 | `var` reassignment | ✅ | ✅ | `16_loops/var_reassign` |
| 18 | Type variable `a -> a` | ✅ | ✅ | `18_generics/identity` |
| 18 | Repeated type variable | ✅ | ✅ | `18_generics/repeated_variable` |
| 18 | Generic containers | ✅ | ✅ | `18_generics/generic_container` |
| 19 | Real platform (basic-cli) | ✅ | ✅ | `19_platform/basic_cli` |
| 09 | Record update `{ ..r, x: v }` | ✅ | ✅ | `09_records/update` |
| 09 | Record destructuring | ✅ | ✅ | `09_records/destructure` |
| 09 | Record `..rest` pattern | ✅ | ✅ | `09_records/destructure_rest` |
| 14 | Nominal method block | ✅ | ✅ | `14_nominal/method_block` |
| 14 | Method dispatch | ✅ | ✅ | `14_nominal/method_dispatch` |
| 14 | Field default `?? ` | ✅ | ✅ | `14_nominal/field_default` |
| 14 | Optional field `?:` and `.?` | ✅ | ✅ | `14_nominal/field_optional` |

### Pair written, interpreter incomplete

These compile under `roc` but the interpreter cannot run them yet. Each names the
one thing that blocks it.

None. Every pair that exists passes all four outputs and the AST check.

`tests/check_roc.sh --strict` is green.

### Not started

No golden pair yet. Writing the pair is step one, and it is where the real types
get pinned down — see the guide's "Establish ground truth" section.

| # | Feature | Notes |
|---|---------|-------|
| 16 | `continue` | **Not yet implemented in Roc** — langref marks it so, and `roc` crashes on it. No pair can be written. |

---

## Phase 21: gaps against the language reference — DONE

Found by comparing against
[docs/langref](https://github.com/roc-lang/roc/tree/main/docs/langref) and verifying
each form against `roc` nightly-2026-09-03. Langref is partly aspirational, so nothing
below was listed on the strength of the docs alone — every row was run.

All five groups are implemented, with fifteen golden pairs under
`tests/roc/21_langref/` and unit tests in `tests/langref_test.rs`.

### 21a. Silently wrong — done

These parsed, produced a value, and the value was **wrong**. Worse than a missing
feature, because nothing reported a problem.

| Form | `roc` | was |
|---|---|---|
| `1_000_000` (digit separators) | `1000000` | `1` — stopped at the `_` |
| `1.5e3` (scientific notation) | `1500` | `1.5` — stopped at the `e` |
| `"caf\u(e9)"` (unicode escape) | `café` | the escape, literally |

A separator must sit BETWEEN digits, so `scan_digits` accepts `_` only when a digit
follows. `1500.0` prints as `1500`: roc drops a whole float's `.0`.

### 21b. Parse errors — done

| Form | Notes |
|---|---|
| `'a'` grapheme literal | A NUMBER — `'a'` is `97`, `'é'` is `233`. roc has no character type |
| `0..<3`, `1..=3` | Ranges. Bind LOOSER than `+` and `*`, so `1 + 1..<2 * 3` is `2..<6` |
| `UserId.(0)`, `Pair.(1, "two")` | Nominal construction for non-record backings |

A range is **not a list**. `Str.inspect(0..<3)` gives `<opaque>` and a range cannot be
passed where a `List` is wanted, so `Value::Range` is its own variant — building it as
a list would have shown `[0, 1, 2]` and wrongly satisfied a `List` parameter.

`Name.(payload)` is transparent, like `Name.{ ... }`: one value is itself, several are
a tuple. roc still refuses `.1` on the result, which the interpreter does not — see
Known ceilings.

### 21c. Statements — done

| Form | Notes |
|---|---|
| `return` | Early return, on the non-local exit `break` already uses |
| `crash "msg"` | Aborts the program |
| `expect cond` | Assertion; writes to **stderr** and does not abort |
| `dbg expr` | Debug output; stderr as well |

### 21d. Accepted but not checked — done

These did not error, which made them easy to mistake for working: an unknown type name
became a fresh variable that unified with anything, so the annotation was ignored
rather than honoured.

| Form | What was missing |
|---|---|
| `Bytes : List(U8)` type alias | The alias never resolved; `b : Bytes` accepted any type |
| `{ a: Str, .. }` open record | Openness was not modelled |
| `{ a: Str, ..r }` named extension | Same |
| `[Red, ..u]` named open union | The name was ignored; anonymous `..` already worked |
| `where [a.to_str : a -> Str]` | Constraints were not read |
| `a + b` dispatching to a user `plus` | Operators were native; a custom method was ignored |

A type alias is **transparent** — stored as the aliased type itself, not wrapped in
`Nominal`, so `Bytes` and `List(U8)` unify freely. A capital letter is what separates
an alias from a value annotation.

`Type::Record` gained an `open` flag, mirroring the `TagUnion { tags, open }` that was
already there. Unification relaxes in one direction per side: an open record tolerates
extra fields on the other side, and tolerates the other side lacking the rest.

Every roc operator is a method on its left operand:

| Operator | Method | Operator | Method | Operator | Method |
|---|---|---|---|---|---|
| `+` | `plus` | `/` | `div_by` | `<` | `is_lt` |
| `-` | `minus` | `//` | `div_trunc_by` | `>` | `is_gt` |
| `*` | `times` | `%` | `rem_by` | `<=` | `is_lte` |
| `-x` | `negate` | `==` | `is_eq` | `>=` | `is_gte` |

`!=` asks for `is_eq` too and negates it; roc names no separate method. Dispatch is
tried only for records, tags and tuples, so a type defining `plus` cannot hijack
`1 + 2`.

A `where` clause is read for the method names it promises, which the checker then
permits on an unresolved type variable. The constraint itself is roc's to verify — the
interpreter only needs permission to dispatch. The names travel from
`Parser::where_methods()` to `TypeChecker::allow_dispatch()`, so an embedder that skips
that wiring gets the old "cannot dispatch on an unresolved type" error.

### 21e. Parameterized nominals — done

`Wrapper(a) := { item: a }`. The parameters take their type-variable ids **before** the
backing type is parsed, so the `a` in `{ item: a }` is the same variable the parameter
list declared; `Wrapper(Str)` then substitutes by id.

### Found along the way

Two parity bugs that no phase had listed, both surfaced by writing the 21b pairs:

- **`var` mutability was keyed on the `$` sigil.** `$sum = ...` reassigned, `sum = ...`
  shadowed. roc gives `$` no meaning of its own — it is an ordinary identifier
  character — and mutability comes from `var`. A loop accumulating into a plain `var`
  name silently produced zero. Now keyed on the `var` declaration.
- **Top-level definitions were not hoisted.** roc's top level is order-independent;
  this interpreter folded it into nested `Let`s, so a helper had to appear before its
  use. Fixed while running the language's own examples: the outermost scope is now
  shared between closures, so a closure sees every top-level name whenever it was
  bound.

### In langref, NOT yet in the compiler

Do not implement these: no golden pair can be written, because `roc` itself rejects them.

| Form | Status |
|---|---|
| `list[i]` subscript | Listed as a postfix operator; `roc` parses it as two expressions |
| `continue` | The docs mark it "not yet implemented" |
| Dictionaries and Sets | `dictionaries-and-sets.md` is a TODO stub |
| Iterators | `iterators.md` is a TODO stub |

---

## Phase 22: the language's own examples

All 28 examples from <https://www.roc-lang.org/examples/> live under
`tests/roc/examples/`, copied verbatim from
[roc-lang/examples](https://github.com/roc-lang/examples). They are not written for
this interpreter — they are the outside-in check on everything the phases built.

```bash
tests/check_examples.sh          # every example, roc vs rocflight
tests/check_examples.sh FizzBuzz # just one
```

Each example is run under both `roc` and `rocflight` and the **combined stdout and
stderr must match byte for byte**. Modules with no entry point are run with `roc test`
against `rocflight --test`, which reports the same `expect` tally.

**Nine cannot be compared**: `roc` itself refuses them with the installed compiler —
eight need a basic-cli platform or package built for a different compiler version
("roc version mismatch"), and `ImportFromDirectory` is rejected outright ("the value
hello is not exposed by the module Dir/Hello"). They are copied and listed as skips.

**12 of the remaining 19 match.** The seven that do not are named in
`tests/check_examples.sh`, each with the subsystem it needs — Dict, Set, the Json
package, the encoder framework, record-builder syntax, `Dec`, and roc's fractional
default for unconstrained number literals.

### What the examples exposed

Every one of these was a real bug, found only because the examples are written by
someone who was not thinking about this interpreter:

| Bug | Was |
|---|---|
| Comments are trivia EVERYWHERE | `skip_whitespace` skipped whitespace only, so a comment between a binding's `=` and its value was a parse error |
| A top-level expression statement ended the parse | Everything after the first `expect` was silently discarded — a module of three tests reported one |
| `expect`s ran in source order | roc loads the whole module first, so an `expect` may call a function declared below it |
| Block-local recursion | A closure captured its environment before its own binding existed, so `go` could not call `go` |
| The top level was not order-independent | A closure captured a deep copy, freezing the top level as it stood; now the outermost scope is SHARED |
| 8 MB of stack | A tree-walker spent many Rust frames per Roc call and died at ~200 levels; the LeastSquares example recurses 501. The register VM's frames are a `Vec`, so the reservation is gone |
| An annotated function could not see its own annotation while its body was checked | `hanoi` calling itself produced an unresolved type |
| `concat` was hardcoded to `Str` | `List.concat` returns a List; the receiver decides |
| Multi-byte characters in string literals | `byte as char` turned `σ` into mojibake |
| `Str.inspect` did not escape | roc renders `"` as `\"` and `\` as `\\` |
| Multi-line record and tag-union TYPES | The type parsers skipped inline whitespace only, so a type written one field per line stopped at the first newline |

### Syntax the examples added

Field punning `{ name, age }` (a comma marks it — roc reads a lone `{ x }` as a block),
bare record patterns `|{ x, y }|`, `Name.(payload)` patterns, multiline strings
(`\\`-prefixed lines, raw except for `${}`), trailing commas in argument lists,
`import "file" as name : Str` ingestion, local module imports with `exposing`,
parameterised type aliases `Parser(a) : ...`, `where` on a continuation line,
`expr ? |err| Mapped(err)`, builtins as values (`xs.map(Str.inspect)`), the `Try`
methods (`map_ok`, `map_err`, `with_default`, `on_err`), iterators, custom
`to_inspect`, and a long tail of `Str`/`List`/numeric builtins.

---

### Known ceilings

Deliberate, and each one diverges from `roc` only where no golden pair can see it.

- **`.1` on a nominal over a tuple.** roc keeps the nominal opaque to tuple and field
  access even though `Str.inspect` sees through it; here the value simply IS the tuple,
  so `p.1` works. Catching it needs the checker to reject access through a `Nominal`,
  which would also have to keep `.{ }` record nominals readable.
- **`var` names are tracked in one flat set.** A `var x` in one function makes a later
  `x = e` in another read as a reassignment rather than a shadow.
- **`where` constraints are not verified**, only read for the names they promise. roc
  checks them at every call site.
- **An unconstrained number literal stays an integer.** In roc it defaults to a
  fractional type, so `a = 7` then `a.to_str()` prints `7.0`. Matching needs the
  checker's choice to reach the evaluator — a typed lowering pass.
- **`Dec` is `f64`.** roc's fixed-point decimal carries more digits.
- **Dispatch on an unresolved receiver is only rejected at the top level**, not inside
  a function body. A body is checked before any call site is seen, and may dispatch on
  the result of a builtin this interpreter does not model.
- **An iterator over a LIST is that list.** `.iter()` on a range keeps it a range —
  which also means `Str.inspect` shows `<opaque>`, as roc does — and the builtins that
  only walk their elements walk it without building one. What is still eager is
  `map`: it produces its output list rather than fusing into its consumer, so
  `xs.map(f).fold(g)` makes one intermediate list.
- **Custom `to_inspect` is picked by trial.** Values carry no nominal tag at runtime,
  so each candidate is applied and the first that returns a Str wins.
- **Checked arithmetic never fails.** `*_try` always gives `Ok` and `*_saturated` is
  the plain operation; there is one integer and one fractional representation, so
  there is nothing to overflow.

---

## Order

Every phase through 22 is implemented: 98 golden pairs pass under `--strict`, 439 Rust
tests pass, and 12 of the 19 comparable language examples match `roc` byte for byte.
What remains is listed as Known ceilings above, plus the seven subsystems named in
`tests/check_examples.sh`.

**Done:** nested calls as arguments. Arguments were parsed with
`parse_primary_expr` (a single atom), so `I64.to_str(inc(41))` failed. Now
`parse_or_expr`. Turned three rows green.

**Done:** records, `Bool`, `//` and `%`. Turned seven rows green. Notes worth
keeping:

- Comparison and logical operators now yield `Bool`, not `Int(0/1)`. Eighteen Rust
  tests asserted the integer model and were rewritten.
- `Str.inspect` sorts record fields **alphabetically**, recursively. Verified against
  `roc` before implementing — source order would have been the natural guess.
- Records and blocks are both `{ ... }`. A record field is `name: value`; a block
  annotation is `name : Type`. The space before the colon is the discriminator, and
  every site accepting braces must share that decision (`Parser::parse_braced`) —
  a lambda body called `parse_block` directly and so failed on `|n| { v: n }`.
- Field access is **postfix**, so it works on any receiver: `r.x`, `f(x).v`,
  `{ a: 1 }.a`. `x.y` is field access only when the receiver is lowercase; Roc
  capitalises modules and types, so `Str.inspect` stays a module member.

**Done:** `if` / `else` (phase 6). Four rows. Notes worth keeping:

- `if` is an **expression**, and `else` is **mandatory** — roc rejects a bare `if`
  with "The second branch of this if does not match the previous branch". The
  interpreter rejects it at parse time.
- The condition must be a `Bool`; roc says so outright ("This if condition must
  evaluate to a Bool"). A number will not do.
- `else if` is **not** a separate construct: it is an `If` whose else branch is
  another `If`. Parsed by recursion, no extra AST node.
- **Function application requires no whitespace before `(`** — roc rejects `f (1)`.
  That rule is load-bearing: without it the condition in
  `if n > 0 (if n > 10 …) else …` swallows the parenthesised then-branch as a call
  on `0`. Field access does *not* share the rule — `r .x` is accepted.
  `Parser::preceded_by_whitespace` recovers the distinction by looking back, since
  `parse_primary_expr` has already consumed the whitespace.

Phases 11, 12 and 17 are the hard ones. 19 (platform loading) is the biggest, and
nothing in `all_syntax_test.roc` needs it — the default host covers `echo!`.

---

## Things that are not phases

Recorded so nobody "implements" them:

- **`foo!`** — the `!` is part of the identifier. Names are looked up verbatim. Not a postfix operator, despite an earlier commit here claiming so; LSP hover spans `echo!` chars 4–9 inclusive of the `!`, and completion returns the literal label `echo!`.
- **`=>`** — the effectful arrow and the `match` arm separator. Never becomes `->`.
- **`module [...]` headers** — deprecated upstream in favour of type modules. Do not add support; do not write new ones.
- **`Result`** — not a type in scope. The nominal type is `Try`.
- **`Str.inspect` on unannotated numbers** — Roc prints `42.0` for an integer literal nothing constrains, because it defaults to a fractional type, but `42` once annotated `I64`. The interpreter always prints `42`. Annotate numbers in any test that inspects them; the golden pairs do, and a mismatch here would surface as the sugared and desugared `roc run` outputs disagreeing — which the pair gate catches.
**Done:** tag unions (phase 12). Five rows. Notes worth keeping:

- A tag literal is a **one-tag union**: `Red : [Red]`, `Foo(1, "a") : [Foo(I64, Str)]`.
- Unifying two unions **merges** their tags, so `if b Red else Green` is `[Green, Red]`.
  `unify` reports success or failure and cannot return a type, so `TypeChecker::join`
  does the merging. `match` arms will need it too.
- Union members are **sorted by tag name**, so declaration order cannot affect unification.
- Payload **arity and types are checked** where a tag appears on both sides: `Foo(1)`
  vs `Foo(1, 2)` and `Foo(1)` vs `Foo("s")` are both rejected.
- `Ok`/`Err` need no special handling — `Try(a, b)` is `[Ok(a), Err(b)]`.
- `[Red, Green, ..]` openness applies to **parameter** positions. A value binding
  annotated with an open union still rejects an unlisted tag.

**Done:** `match` (phase 11). Six rows, plus the last blocker on the `?` pair's
desugared half. Notes worth keeping:

- Arms are newline-separated and **order matters**: the first pattern that matches,
  whose guard also holds, wins. A comma between arms is allowed but optional.
- `_` is the wildcard. `_unused` is an ordinary **binding** that documents being
  unused — verified against roc, which returns the bound value.
- A **false guard falls through** to the next arm rather than failing the match, and
  the guard sees its pattern's bindings.
- Pattern bindings are collected into a list and only bound once the whole pattern
  matches. A nested pattern can bind several names and then fail on the last element;
  without that, stale bindings would leak into the following arm.
- `TypeChecker::join` is reused for arm bodies, so arms yielding different tags give
  the union — the same machinery tag unions needed.
- **List patterns are absent** (`[]`, `[x, ..]`, `[1, .. as tail]`): they need lists,
  a later phase.

**Fixed on the way:** string interpolation could not contain a string literal.
`"${render(Foo(42, "answer"))}"` ended the outer string at the inner quote. The
scanner now tracks brace depth inside `${...}` and skips nested literals, honouring
escapes. Pre-existing bug, exposed by the phase-11 test files.

**Done:** `?` and `??` (phase 17). Four rows. Notes worth keeping:

- **`?` and `??` are desugared in the PARSER, not the text-level desugarer.** `??`
  needs its operand's extent, which means expression parsing; `?` has to move the
  rest of the block into the `Ok` arm, and locating "the rest of the block" in raw
  text is not something string substitution can do reliably. The parser already folds
  block statements from the end, so the continuation is exactly what it holds.
  The four placeholder passes that claimed to be the site were deleted.
- `x = expr?` becomes `match expr { Ok(x) => <rest>, Err(e) => Err(e) }`. `_ = expr?`
  uses a wildcard in the Ok arm but still short-circuits.
- `?` on a block's **final expression is rejected**. It unwraps, so the body would
  yield the payload rather than a `Try` — roc rejects it as a type error, and there is
  no continuation to move.
- `??` is the **loosest** operator: `x ?? 1 + 2` is `x ?? (1 + 2)` = 3. It wraps the
  or-level parser, so every existing call site got it without being touched.
- **`.?` segfaults the roc compiler** (nightly-2026-09-03). Blocked upstream; no
  golden pair can be written, so it is not implemented.
- The error is bound to the fixed name `e`, so a continuation referring to an outer
  `e` would see the propagated error instead. Marked `ponytail:` — needs a gensym.

**Watch out:** `roc check` **exits non-zero on warnings**, so "compiles cleanly"
already means warning-free. Two `??` pairs initially failed on "this match value is
known at compile time" — a constant scrutinee. Putting the parse behind a function
parameter fixed it. Keep test inputs non-constant when the desugaring produces a
visible `match`.

**Done:** lists and their builtins (phase 15), plus the list patterns they unblocked
in phase 11. Seven rows. Notes worth keeping:

- **`List.len` returns U64 in roc.** Mixing it with `I64` arms in a match is a type
  error — one pair had to change its return type to `U64` because of it. The
  interpreter does not track numeric types, so it returns a plain integer.
- Argument order follows roc: `List.map(list, fn)` and
  `List.fold(list, initial, fn)`, with the accumulator as the callback's **first**
  parameter. `fold` with subtraction is the test that catches a swap.
- `Str.inspect` renders `[1, 2, 3]` and recurses into nested lists.
- Equality is element-wise **and** length-sensitive.
- `..` in a list pattern may sit at the **end, middle, or start**, matches **zero or
  more** elements, and `.. as name` binds the skipped middle as a list — verified
  against roc, including `[2, .., 1]` matching `[2, 1]` and `[.., 5]` NOT matching
  `[5, 1]`. At most one `..` per pattern.
- `List.len` is also why `[9, .. as tail] => 77 + List.len(tail)` forces the whole
  match to be `U64`.

**Refactor on the way:** lambda application was duplicated in two `eval` call sites.
Factored into one `apply(func, args)`, which the `List.map`/`fold` callbacks also
needed. `Str.len` does not exist in roc — do not add it.

**Done:** tuples (phase 10). Six rows. Notes worth keeping:

- **`(1)` is grouping, not a one-tuple.** roc has no one-tuple, so the paren parser
  decides on the comma: `(a, b)` is a tuple, `(a)` is the expression `a`.
- `.0` is **zero-based** and postfix, so it works on any receiver — `mk(5).1` included.
  It shares the postfix slot with `.field`, distinguished by digits vs an identifier.
- Tuples are positional, so unlike records their type does NOT sort, and they unify
  only at equal arity.
- **Destructuring desugars differently by position**, which is the interesting part:
  - Inside a block → a **one-arm match**, reusing existing machinery. Scoping the arm
    is correct there.
  - At the top level → **indexing**, because a top-level binding must NOT be scoped.
    The continuation is the rest of the file, which defines `main!`, and a match arm
    pops its scope on the way out — so the match form made `main!` unfindable.
- Ceiling: a **refutable** top-level pattern (`(1, b) = pair`) is rejected. roc allows
  it; it needs a match, which top level cannot use. The error points at the block form.
- A statement may legitimately start with `(`, so the destructuring check backtracks
  when no `=` follows — `{ (1 + 2) }` is a grouped expression, not a binding.

**Done:** type annotations in the AST. Not a phase — one gap that was the root cause
of three separate ceilings, all now closed:

1. **Identifiers carry a type.** `synth` has a scoped environment; every `Expr::Ident`
   used to be a fresh variable. `let x = 42 in x` now infers `I64`, not `$0`, and
   `x = 42` then `x(1)` is rejected.
2. **Closed tag unions are enforced.** `c : [Red, Green]` rejects `c = Blue`. A union
   from an annotation is **closed**; one inferred from a tag expression is **open**
   (`[Red, ..]`), since a tag literal only says "at least this tag".
3. **`match` exhaustiveness is checked** when the scrutinee's union is closed. A
   wildcard or binding arm completes a match; alternatives count; a **guarded** arm
   does not, because it may not run — verified against roc, which rejects the same
   program.

How it works: `capture_type_annotation` parses the annotation instead of skipping it
and stashes it until the matching binding claims it, so `Expr::Let` carries
`annotation: Option<Type>`. An annotated binding is **checked** against its type
rather than inferred (`TypeChecker::check`), which is also how a lambda's parameters
get their declared types — and therefore how a `match` on a parameter learns its union.

Four things fell out that were invisible before:

- **Record field types, list element types, tuple arity and payload types** are now all
  checked against their annotations.
- **`Bool.True` was typed as a function.** `synth` treated every `Module.name` as a
  function; the evaluator knew better. The two disagreed and nothing noticed until
  annotations were checked against.
- **Arithmetic returned `I64` unconditionally**, so `quot : F64` rejected
  `quot = 7.0 / 2.0`. It now returns the operand type.
- **Numeric literals are polymorphic** — `255` satisfies `U8` as well as `I64`.

Two deliberate divergences from roc, both permissive rather than strict:

- **Integer widths are not distinguished** in unification. The evaluator has one
  integer representation, so enforcing widths in the checker would be theatre. `roc
  check` is the authority, and every pair goes through it.
- **`[Red, Green, ..]` on a value binding** accepts an unlisted tag here; roc restricts
  openness to parameter positions.

**Not** fixed by this, despite my earlier grouping: **refutable top-level
destructuring**. That needs a runtime equality check at top level (or a non-scoping
match), and no amount of type information changes it. It was mis-grouped.

**Harness note:** `--ast-only` prints an AST line and a `:: type` line; the harness
compares only the AST. Annotations are not sugar, so a desugared file's declared type
is legitimately more specific than the sugared file's inferred one
(`List(Str) -> ...` versus `$0 -> ...`) while the structure is identical.

**Done:** nominal types (phase 14). Three rows. Notes worth keeping:

- `Name := backing` is **nominal but not opaque**. Verified against roc: the plain
  backing value IS accepted where the nominal is expected (`f({ x: 1 })` for
  `f : Point -> _`), yet two *different* nominals with identical backing do not
  interchange. Unification mirrors that — same name unifies backings, nominal against
  anything else falls through to the backing.
- **Values carry no nominal wrapper.** `Str.inspect` on one shows the bare backing
  record, and `Animal.Dog(x)` builds the same value a bare `Dog(x)` would. The
  qualification is type-level only, so the evaluator needed nothing new.
- **Exhaustiveness reaches through a nominal** to its backing union — roc checks these
  too, and the check now unwraps before looking.
- **Lambda parameters are patterns**, not just names. `|Point.{ x, y }|` needs it, and
  a pattern parameter becomes a one-arm match on a generated name — the same shape a
  destructuring binding uses. `|(a, b)|` works for free.
- **Sub-parsers must inherit parser state.** Each `${...}` gets its own `Parser`, which
  had no nominal declarations, so `Animal.Dog(x)` inside an interpolation parsed as a
  qualified CALL. Anything the parser accumulates has to be threaded down.

Deferred, with reasons rather than as unfinished work:

- **`::` opaque types** — within one file roc does not distinguish them from `:=`
  (both allow field access, both accept the plain backing). Opacity only matters across
  module boundaries, which the interpreter does not have. Accepted as a synonym.
- **`.{ ... }` method blocks** — methods need static dispatch (phase 20). The block is
  consumed and ignored so the rest of the file still works.
- **`field : T ?? default` and `field ?: T`** — reading an optional field needs `.?`,
  which segfaults the roc compiler.

**Done:** static dispatch (phase 20). Four rows. Notes worth keeping:

- `receiver.method(args)` becomes `Module.method(receiver, args)` — the receiver is
  the **first** argument, which is why roc's builtins take their subject first.
- **Parens separate a method call from a field read.** `s.is_empty` reads a field,
  `s.is_empty()` calls a method, so the check has to happen before committing to a
  `FieldAccess`.
- The **checker** resolves the module from the receiver's type and rejects an
  unresolved one — roc does the same ("trying to dispatch a method named to_str on an
  unresolved type variable"). The **evaluator** resolves the same thing from the
  value's runtime kind; the two agree because a value's kind follows its type.
- **Chaining needs result types.** `xs.len().to_str()` dispatches on what `len`
  returned, so a bare type variable there makes the second dispatch impossible. Most
  results come from the method name, but `fold` returns its *accumulator* — its type
  comes from the first written argument, which a name-only table cannot express.

**Bug this exposed:** `StrInterp` synthesised as `Str` **without checking its parts**,
so every error inside `${...}` went unreported. That is what hid broken chained
dispatch in this project's own golden pair — `list_methods` passed while
`xs.len().to_str()` was in fact untypeable. Interpolated expressions are now checked.

**Refactor on the way:** builtins took unevaluated `&[Expr]` and evaluated arguments
arm by arm. They now take `Vec<Value>` (`call_builtin_values`), with a shim that
evaluates first — dispatch already holds the receiver as a value, so without this every
builtin would have needed a second entry point.

**Not done: nominal method blocks** (`Secret :: {...}.{ unlock = ... }`). Phase 14
deferred these to "when static dispatch lands", but that was the wrong diagnosis. The
blocker is not dispatch — it is that **values carry no nominal identity**: roc erases
it, as `Str.inspect` showing the bare backing record proves. So roc must resolve
nominal methods entirely at COMPILE time, whereas this interpreter resolves dispatch at
run time from the value's kind. Supporting them needs either a runtime nominal wrapper
or a checker→evaluator channel that records the resolved method at each call site.
Neither is small, and neither is "static dispatch".

**Done:** pipelines (phase 13). Four rows. Notes worth keeping:

- `x |> f` is `f(x)`, and `x |> f(a)` is **`f(x, a)`** — the piped value is
  **prepended** to whatever arguments were written, the same convention static
  dispatch uses. Subtraction is the test that catches a swap.
- Left-associative: `x |> f |> g` is `g(f(x))`.
- **`|>` binds TIGHTER than every binary operator**, which is the opposite of most
  languages, where a pipe is the loosest thing in an expression. Verified against roc:
  `1 + 2 |> double` is `1 + double(2)` = 5, and `2 * 3 |> inc` is `2 * inc(3)` = 8.
  So the pipe level sits between the multiplicative operators and the call level, not
  at the top of the chain.
  - Watch the test numbers: `2 * 3 |> double` gives 12 under *both* readings, so it
    proves nothing. A non-doubling function is needed to tell them apart.
- `|`, `||` and `|>` all start the same way, so the match-alternative separator has to
  exclude `|>` explicitly.

**Made consistent on the way:** `xs.len().to_str()` worked while `List.len(xs).to_str()`
did not — the checker resolved a builtin's result type for dispatch but not for a
qualified call. Both spellings now share one `builtin_result`, so all three forms
(`xs.len()`, `List.len(xs)`, `xs |> List.len()`) chain alike. Confirmed against roc.

**Done:** unary minus (phase 07). Three rows. Notes worth keeping:

- **`-x` IS `x.negate()`.** roc lowers it to that method — which is why `-s` on a Str
  fails with "This negate method is being called on a value whose type doesn't have
  that method" rather than a syntax error. Building a `Dispatch` reuses phase 20
  wholesale and gives the same diagnostic for free. The golden pair's desugared half
  writes `n.negate()` literally, and the two build an **identical AST**.
- The operand is the whole **postfix chain**: `-r.v` is `-(r.v)`, and `-n.to_str()`
  tries to negate a Str.
- Precedence, tightest first: postfix, `|>`, unary minus, then the binary operators.
  `-n |> inc` is `-(inc(n))` = -6, not `inc(-n)` = -4 — the readings differ, so the
  numbers in that test do real work.
- A negative **literal** (`-5`) stays one token rather than becoming a negate call.

**Bug this exposed:** a `-` with whitespace before it and none after is a unary
negation, not a subtraction — and without that rule, a value at the end of one line
followed by a line starting `-x` read as subtraction **across the newline**
(`n = 5` then `-n` became `5 - n`). roc rejects `m -n` and `m-n` outright for exactly
this reason. The gap has to be found by looking BEHIND, since `parse_primary_expr`
has already consumed the newline — the same trap the call parser hit in phase 06.

**Known divergence:** roc rejects a `-` bound tightly to an identifier (`m-n` is a
parse error there, while `m - n` and `10-3` are fine). This interpreter accepts it as
subtraction — more permissive, which is the safe direction, and no golden pair can
depend on it because every pair passes `roc check`.

**Done:** loops and mutable bindings (phase 16). Four rows. Notes worth keeping:

- **`$` is part of the identifier.** `$sum` and `sum` are different names in roc, the
  same way a trailing `!` distinguishes an effectful one. `var` also works without the
  sigil; the `$` is convention, not syntax.
- A plain binding **cannot** be reassigned — roc reports a redeclaration. Only a `var`
  may appear on the left of an assignment.
- Assignment **updates in place** (`Environment::assign`). Binding would shadow, and a
  loop body runs in its own scope, so the update would be discarded when that scope
  pops and the value read after the loop would be unchanged.
- Loops are **expressions** whose value is `{}`, so they can be bound
  (`y = for n in xs { ... }`) as well as used as statements. Handling them in
  `parse_primary_expr` covers both and needs no statement special case.
- `break` rides the **error channel** (`EvalError::break_signal`), which keeps the
  evaluator's signature as `Result<Value, EvalError>` instead of threading a
  control-flow enum through every arm. A loop catches it; anything else propagates it,
  so a stray `break` surfaces as an error rather than vanishing. The signal's message
  starts with a NUL so it cannot collide with a real one.

**Bug this exposed:** the block fold **discarded the final statement's binding
target**, so a loop body whose only statement was an assignment did nothing —
`for n in xs { $sum = $sum + n }` became `for n in xs { $sum + n }`, and `while` hung
forever because its counter never advanced. `break_early` passed throughout, because
its assignment is not the last statement. A block ending in an assignment now still
runs it, with the block's value being unit.

**Not implemented: `continue`.** It CRASHES the roc compiler on nightly-2026-09-03
("Please report this issue at github.com/roc-lang/roc/issues"), so no golden pair can
be written against it. Blocked upstream, like `.?`.

**Done:** generics (phase 18). Three rows. It turned out to be **two** bugs, not the
one I expected:

1. **A repeated type variable was not the same variable.** Each lowercase name got a
   new fresh id every time it was parsed, so `pair : a, a -> a` wrongly accepted
   `pair(1, "s")`. The parser now keeps a per-annotation name→id map, cleared between
   annotations so the `a` in one signature is unrelated to the `a` in the next.
2. **No generalisation.** `identity : a -> a` used at Str and then at I64 failed,
   because the first use pinned `a`. An annotated binding's type variables are now
   universally quantified, and every use **instantiates** fresh ones.

**The bug underneath both:** the parser and the checker allocate type-variable ids
from the **same number space**, so an annotation's `$1` was literally the checker's
first fresh variable and unified with something unrelated. That surfaced as
`List(a) -> List(a)` failing with "Cannot unify List(I64) with I64" — two unrelated
types meeting through a shared id. Instantiating annotation types on ingest keeps the
parser's ids out of unification entirely, which fixes the collision and delivers
generalisation in the same move.

Only ANNOTATION variables are quantified; an inferred type stays monomorphic. That is
weaker than full let-polymorphism and deliberately so — it is what the annotations work
made available, and `roc check` remains the authority.

**Watch out:** `count : List(a) -> I64` makes roc report a *compile-time crash*, not a
type error. `List.len` returns U64, and the annotation has to say so.

**Done:** real platform loading (phase 19). One row. Notes worth keeping:

- **`roc` already downloads, verifies and extracts dependencies**, so none of that is
  reimplemented. The interpreter reads what `roc` left in
  `~/.cache/roc/packages/<HASH>/`, where `<HASH>` is the URL's filename minus its
  archive extension — a URL maps to a directory with no network access and no hashing
  of our own. `roc` is the authority on cache layout and archive integrity; a second
  implementation would be one more thing to keep in step.
- The archive is **`.tar.zst`** now. The plan's "download 14MB brotli-compressed tar"
  was Rust-era; both extensions are recognised, and both cache layouts
  (content-addressed flat, and the old mirrored URL path) are checked.
- A platform root declares `requires`, `exposes`, `packages`, `provides` and `hosted`.
  The `requires` signature **contains `{}`**, so reading it needs brace matching — a
  scan to the first `}` truncates it.
- An exposed module declares its members inside `Name :: [].{ ... }`. Anything after
  that block is **private**: `Stdout.roc` closes the block and then defines
  `widen_stdout_err`, which scanning the whole file would wrongly export.
- The app header's dependency map distinguishes `alias: platform "URL"` from
  `alias: "URL"`, and `roc: "nightly-..."` pins the compiler rather than naming
  something to fetch — it resolves to no archive and is skipped.

**The architectural limit, stated plainly:** a platform's `hosted` functions live in
its **compiled host**, which an interpreter cannot call. So an effect runs
only where this interpreter supplies its own implementation (`Stdout.line!`,
`Stdout.write!`, `Stderr.line!`, `Stderr.write!`). Anything else the platform declares
is reported as *"provided by the platform's compiled host, which this interpreter
cannot call"* — deliberately distinct from "unknown function", because the platform
really does provide it and the gap is ours.

**Bug this exposed:** after parsing the header's `{ ... }` map the cursor already sat
on the next line, and the caller skipped to the next line *again* — swallowing the
first `import`. It only showed up with a header present, which is why the no-header
case kept passing.

`--show-platforms` reports what resolved: sources directory, module count, hosted-effect
count, and the `requires` signature.

**Done:** record update and destructuring (phase 09's remainder). Three rows.

**The docs were wrong about the syntax.** This table said "update `{ r & x: 5 }`" —
that spelling is **rejected** by roc. It appears in `all_syntax_test.roc` only inside a
commented-out TODO block, which is where the mistake came from. The real spelling is
`{ ..base, field: value }`. Checking the reference file's *live* code rather than its
comments would have caught it.

Other notes worth keeping:

- An update builds a **new** record; the base is untouched. It cannot ADD a field —
  updating one the record lacks is an error.
- **A bare name puns in a PATTERN but not in a LITERAL.** `{ name } = r` binds `name`,
  but `{ name }` as an expression is a BLOCK whose value is `name` — verified against
  roc, which prints the string rather than a record. That asymmetry is why
  `looks_like_record` still requires `name:` to see a literal.
- `..rest` binds every field not named, producing a record with **fewer** fields — which
  is what makes `{ email: _, ..rest }` a way to remove one. The checker computes the
  remainder type by subtracting the named fields.
- A record pattern with `..rest` must not type as a closed record: it matches a record
  with *more* fields than it names.

**Ceiling:** `..rest` is rejected in a TOP-LEVEL destructuring. Building the remaining
record needs a match, which a top-level binding cannot use for the scoping reason
recorded under phase 10. The error says so and points at the block form.

**Done:** nominal method blocks (the rest of phase 14). Two rows.

**My earlier deferral reasoning was half wrong.** Phase 14 said methods "need static
dispatch, a later phase", and phase 20 then said the blocker was really that values
carry no nominal identity. Both were overstated: a method block holds **ordinary Roc
functions**, which this interpreter can evaluate. Parsing them as `Type.method`
bindings makes `Type.method(x)` a plain name lookup, and only the *dispatch* form
`x.method()` needs anything special.

- A method block's bindings are wrapped around the whole program, so `Secret.reveal` is
  an ordinary binding. Only the **outermost** `parse_expr` wraps — it is re-entrant (a
  lambda body goes through it), and wrapping at every level nested the methods inside
  the first method's own body.
- A method's annotation lives **inside** the block, and is claimed with the binding.
  Without it `show = |c| c.n.to_str()` has an untyped `c` and cannot dispatch on `c.n`.
- The **checker** resolves a nominal receiver's method by TYPE — `c : Counter` means
  `Counter.method` — which is real static dispatch. The **evaluator** cannot: values
  carry no nominal tag, so it searches by method name and **reports ambiguity** when
  two types define the same name rather than guessing.
- `Type.method` had to be resolved from the environment in three places the qualified
  path already handled as builtins: bare reference, call, and dispatch. Missing any one
  of them broke chaining rather than failing outright.
- Field access and tuple indexing now **reach through a nominal** to its backing, which
  every method body relies on.

**`::` is not opaque within one file** — a raw record is accepted where the nominal is
expected, exactly as with `:=`. Verified against roc. Opacity needs module boundaries
this interpreter does not have, so the two stay synonyms.

**Done:** field defaults and optional fields (the last of phase 14). Two rows.

**A correction: `.?` is not blocked, and never was.** Earlier notes said it "segfaults
`roc` — blocked upstream". It segfaults when **misused** on an ordinary field of a plain
record, which is not what it is for. On a nominal's `?:` field it works correctly. The
phase-17 probe that produced that claim tested the wrong construct, and the wrong
conclusion then sat in the docs blocking two features.

The two are genuinely different things, which the old note conflated:

| Declaration | Meaning | How to read it |
|---|---|---|
| `name : Type ?? default` | **Defaulted** — omitting it substitutes the default | plain `.name`; it is always present |
| `name ?: Type` | **Optional** — it may genuinely be absent | `.?name`, giving `Ok(v)` or `Err(MissingField)` |

- A default is filled **at construction**, so the constructed record really has the
  field and the type matches without any special case.
- An optional field needed `Type::Optional`, and record unification had to move from a
  positional walk to a walk **by name** — the two field lists can now differ in length.
  A field missing from one side is accepted only when the other declares it optional;
  a missing *required* field and an unexpected extra are both still rejected, and there
  are tests for each.
- Both are only allowed on a nominal's backing record.

**Same sub-parser trap, third time:** a construction inside `${...}` gets its own
`Parser`, which inherited `nominals` but not the field defaults — so omitted fields were
silently left out there while working everywhere else. Anything the parser accumulates
has to be threaded into sub-parsers.

- **Whitespace before `(`** — significant. `f(1)` is a call; `f (1)` is an error in roc, and the interpreter follows. Whitespace before `.` is *not* significant.
- **Sub-expression parsing** — anywhere an expression can appear (call arguments, tag arguments, top-level binding values, block statements) it must be parsed with `parse_or_expr`, the top of the precedence chain. Three separate bugs came from using a lower rung: arguments and top-level bindings could not hold operators or nested calls. Reach for `parse_primary_expr` only when a bare atom is genuinely all that is legal.
- **Blocks** — no `Block` AST node. The parser lowers `{ a = 1 \n f(a) \n expr }` to nested `Expr::Let`, binding non-binding statements to `_`.

---

## Regenerating the status table

```bash
cd /home/brian/Code/rocflight
cargo build --quiet
tests/check_roc.sh --strict
```

`ok` rows are done; `FAIL` rows name which side differs. For the raw four values of
one pair:

```bash
F=tests/roc/04_operators/arithmetic
for f in $F.roc $F.desugared.roc; do
  printf '%-42s roc=%-20s int=%s\n' "${f##*/}" \
    "$(roc run "$f" 2>&1)" \
    "$(./target/debug/rocflight "$f" 2>&1 | grep -v '^\[Desugaring\]')"
done
```

Update the tables from that output. A row claiming ✅ that the harness disagrees with
is worse than no table.
