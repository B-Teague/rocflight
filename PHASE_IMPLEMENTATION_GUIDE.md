# Phase Implementation Guide

How to add a syntax feature to the interpreter. This is the protocol; the feature
list lives in [IMPLEMENTATION_PHASES.md](IMPLEMENTATION_PHASES.md) and the
architecture in [ROC_INTERPRETER_PLAN.md](ROC_INTERPRETER_PLAN.md).

**Verified against:** `roc` nightly-2026-09-03-62fcb65. Every claim below was
checked by running the compiler, not from memory. Re-verify when the nightly moves.

---

## The golden-pair rule

**Every single syntax feature gets its own pair of `.roc` files, and both must
compile cleanly with `roc`.**

```
tests/roc/<NN>_<phase>/<syntax>.roc              # sugared — how a person writes it
tests/roc/<NN>_<phase>/<syntax>.desugared.roc    # desugared — explicit types, no sugar
```

### All four outputs must be identical

```
        roc run <sugared>   ═══   roc run <desugared>
              ║                          ║
              ║                          ║
     rocflight <sugared>   ═══   rocflight <desugared>
```

Four programs, one output. Each edge of that square catches a different class of bug:

| Edge | Catches |
|---|---|
| `roc` sugared ═ `roc` desugared | The desugaring changed the meaning of the program. |
| `rocflight` ═ `roc`, on the **sugared** file | The interpreter is wrong about the surface syntax. |
| `rocflight` ═ `roc`, on the **desugared** file | The interpreter is wrong about the explicit form — a different code path, and it really does differ. |
| `rocflight` sugared ═ `rocflight` desugared | Falls out of the above, and means the interpreter's own desugaring agrees with the hand-written one. |

**Running the interpreter against the desugared file is not redundant.** It is a
distinct path through the parser, and it found a real bug: top-level bindings parsed
their value with `parse_call_expr`, so `a = 2 + (3 * 4)` failed — while the sugared
file passed, because there the same binding sat inside a block, which uses the full
operator-precedence chain. Sugared-only checking could not see it.

### Same sugar in, same AST out

A pair's two files differ **only in sugar**, so the interpreter must build the
**same AST** and infer the **same type** from both. `tests/check_roc.sh` enforces it
by diffing `--ast-only` output.

This catches what output comparison cannot. Five pairs once had their bindings inside
`main!`'s block in the sugared file but lifted to the top level in the desugared one.
Output matched, types matched, and the ASTs were structurally different — the pair was
testing two different programs. If a desugaring genuinely has to restructure, the
sugared file should be written to match.

Inspect it directly:

```bash
R=./target/debug/rocflight
diff <($R --ast-only F.roc) <($R --ast-only F.desugared.roc)
```

### No state carries between runs

Every run re-reads and re-parses the source. Nothing is cached across runs:
`.rocflight/cache/desugared/` is a write-only dump, never read back, and writing it is
opt-in (`--emit-desugared`). So editing a `.roc` file always takes effect immediately,
and a stale dump can never affect a run.

### The full requirement list

1. `roc check <file>` reports **no errors** — for *both* files.
2. All four outputs above are **byte-identical**.
2b. Both files build the **same AST** and infer the **same type**.
3. The `.desugared.roc` carries **explicit type annotations** on its top-level
   bindings. Making the types explicit is the whole point of the file.
4. Each pair covers **one** syntax feature. Not one phase — one feature. `+` and
   `//` are separate files.

### Running it

```bash
tests/check_roc.sh                          # all pairs
tests/check_roc.sh tests/roc/04_operators   # one phase
tests/check_roc.sh --strict                 # interpreter parity required
```

Interpreter flags for working on a pair:

```bash
R=./target/debug/rocflight
$R F.roc                    # run it (quiet: no dump, no progress noise)
$R --ast-only F.roc         # print the AST and inferred type, do not run
$R --show-ast F.roc         # print both, then run
$R --show-desugared F.roc   # print the desugared source
$R --emit-desugared F.roc   # also write the dump under .rocflight/cache/
```

Two gates, because a pair can be correctly written while the interpreter is still
catching up:

- **PAIR** — both files check, the two `roc run` outputs agree, annotations present.
  Failing this means *the test files are wrong*. Always fatal.
- **INTERP** — `rocflight` matches `roc` on **both** files. Failing this means *the
  feature is not implemented yet*. Reported as `PEND`; fatal only under `--strict`.

So `ok` means all four outputs match. `--strict` is the definition of done for a
feature, and what a finished phase must pass; the default mode is what you run while
writing the pair, before any Rust exists.

### No annotations in the sugared file

The sugared file states **no types at all**. Every type is inferred from the syntax, and
the language stays strongly typed while it happens. The desugared sibling is where the
types are written down. That is the whole distinction between the two files.

This is not cosmetic: it is what proves inference actually works. An annotation on every
binding lets a checker coast — it never has to derive anything.

**An unannotated integer literal is a FRACTIONAL type in roc.** `a = 7` then
`a.to_str()` prints `7.0`, not `7`. So an annotation is not always sugar: deleting one
can change a value's type and its printed form, which breaks the pair. Pin the type
through a **use** instead, which is inference rather than declaration:

```text
I64.to_str(x)                 the qualified call fixes x at I64
I64.from_str("42") ?? 0       an I64 source, and it propagates:
                              seeding a fold with it pins the whole list
xs.len()                      U64, straight from the builtin
f(x) where f is annotated     the parameter's type flows into the argument
```

Prefer a pin that shows up in the program's output — an extra `${I64.to_str(...)}` in
the echoed string adds coverage, where a dead `_ = ...` line adds noise.

**Twelve files keep an annotation, for two reasons only.** Both are stated in a comment
at the top of the file:

1. *The annotation is the syntax under test.* `where [a.to_str : a -> Str]`,
   `{ name: Str, .. }`, `[Red, ..u]`, `Bytes : List(U8)`, `Wrapper(a) -> a`, `a -> a`.
   Removing it deletes the feature the pair exists to test.
2. *`roc` itself refuses the file without it.* A top-level `empty = []` is an
   unresolved polymorphic value; a method block needs annotations before roc will attach
   its methods; and an unannotated function whose result is constant makes roc warn that
   a `match` on it is "known at compile time" — and `roc check` exits non-zero on a
   warning.

Anything else keeping an annotation is a bug in the pair, not an exception.

### Why the desugared file must compile

The interpreter emits its desugared output to
`.rocflight/cache/desugared/<path>.desugared.roc` (see `--show-desugared`). If
that output is not valid Roc, it cannot be checked against the real compiler, and
a desugaring bug stays invisible until it shows up as a wrong answer.

So the desugarer **preserves type annotations** rather than deleting them. It used
to strip them so the parser never saw them; that made the emitted file
un-compilable and threw away the types.

The parser now **parses** them rather than skipping them
(`Parser::capture_type_annotation`), and `Expr::Let` carries the declared type. That
is what lets the checker reject a tag outside a closed union, check a `match` for
exhaustiveness, and give every identifier a real type — three ceilings that all came
from annotations never reaching the AST.

### What "desugared" means here

Desugared means sugar removed and types written out — **not** rewritten into
another language, and not normalised beyond recognition. Things that look like
sugar but are not:

| Looks like sugar | Actually |
|---|---|
| `foo!` | The `!` is **part of the identifier**. Nothing to rewrite. |
| `!foo` | Unary logical not. Canonicalises to `Bool.not(foo)`. |
| `=>` | The effectful-function arrow, and the `match` arm separator. Never `->`. |
| `"${x}"` | A primitive string form, not sugar for concatenation. |
| `255.U8` | A type-suffixed literal. Desugars by *lifting the suffix to an annotation*: `small : U8` / `small = 255`. |

Real sugar that does expand: `?`, `??`, `.?`, `?:`, and implicit operator
precedence (made explicit with parens). See [DESUGARING.md](DESUGARING.md).

---

## The reference is partly aspirational

[docs/langref](https://github.com/roc-lang/roc/tree/main/docs/langref) is the language
reference, and the right place to look for what exists. But it describes intent as well
as reality, so **run every form before building against it**:

- `list[i]` subscript is documented as a postfix operator; `roc` parses it as two
  expressions.
- `continue` is documented and marked "not yet implemented".
- `dictionaries-and-sets.md` and `iterators.md` are TODO stubs.
- `{ r & x: 5 }` record update appears in `all_syntax_test.roc` — inside a
  commented-out block. The real spelling is `{ ..r, x: 5 }`, and the commented form is
  rejected. Read the reference file's live code, not its comments.

The same caution applies in reverse: a form that looks broken may just be misused.
`.?` was recorded here as "segfaults roc, blocked upstream" for several phases; it
segfaults only on an ordinary field of a plain record, and works correctly on the
optional field it is meant for.

---

## Verified language facts

Get these wrong and the test files will not compile.

### Entry point

A platformless app — no platform in the header — is what `roc run` links the
built-in default host for. Both of these are valid and current:

```roc
main! = |_args| {          # headerless: `app [main!] {}` is implied
    echo!("hello")
    Ok({})
}
```

```roc
app [main!] {}             # explicit, same thing

main! : List(Str) => Try({}, [Exit(I8), ..])
main! = |_args| {
    echo!("hello")
    Ok({})
}
```

The default host requires **exactly** this entry-point type:

```
main! : List(Str) => Try(_a, [Exit(I8), ..])
```

Annotate anything else and the compiler rejects it. Notably:

- **`Try`, not `Result`.** `Result` is *not* in scope — `roc check` says "The type
  Result is not declared in this scope."
- `=>`, not `->`, because `main!` is effectful.
- `[Ok({}), ..]` is **not** accepted in the annotation; the nominal `Try` is required.

`main!` is also the standard export name for apps with a *real* platform
(`app [main!] { pf: platform "..." }`) — it appears that way ~435 times in the
compiler's own test suite. It is current syntax, not legacy.

### `echo!`

```
echo! : Str => {}
```

Confirmed by `roc experimental-lsp` hover and by the completion item's `detail`.

**`echo!` is provided by the default host, not by the compiler.** It is not a
global builtin: referenced from a type module it fails with "Nothing is named
echo! in this scope." The interpreter models this in
[`src/platform/host.rs`](src/platform/host.rs) — a host-effect table, separate
from compiler builtins.

`echo!` writes its argument with **no trailing newline**, so test files spell
newlines explicitly (`echo!("hi\n")`).

### Modules

`module [foo]` headers are **deprecated**. `roc check` warns: "Type modules
(headerless files with a top-level type matching the filename) are now the
preferred way to define modules." Do not write new `module [...]` headers.

### Numeric stringification

`to_str` dispatches on the numeric type: `I64.to_str(n)`, `F64.to_str(x)`,
`U8.to_str(b)`. A bare `n.to_str()` on an unannotated binding fails with
"trying to dispatch a method named to_str on an unresolved type variable" — annotate
the binding or use the qualified form. The test files use the qualified form.

---

## Adding a feature

### 1. Establish ground truth with the compiler

Do not guess types. Two tools, in order of preference:

**LSP** (exact types for a real file — this is how `echo!`'s signature was found):

```bash
roc experimental-lsp --stdio      # then drive it over JSON-RPC
```

`textDocument/hover` gives the type at a position; `textDocument/completion`
gives a `detail` field per symbol; `textDocument/inlayHint` annotates a whole file.

**The compiler's own error messages**, which name the type it wanted:

```bash
printf 'app [main!] {}\n\nmain! : Str => Str\nmain! = |_| ""\n' > /tmp/t.roc
roc check /tmp/t.roc
#   But the platform requires:
#       List(Str) => Try(_a, [Exit(I8), ..])
```

Deliberately annotating something wrong to read the expected type back is the
fastest way to pin an unknown signature.

### 2. Write the golden pair, and make it green before touching Rust

```bash
mkdir -p tests/roc/09_records
$EDITOR tests/roc/09_records/field_access.roc
$EDITOR tests/roc/09_records/field_access.desugared.roc
tests/check_roc.sh tests/roc/09_records
```

The pair is the specification. If it will not compile, the feature is not yet
understood well enough to implement.

### 3. Implement, in this order

| File | Change |
|---|---|
| `src/ast/mod.rs` | New `Expr` variant — but check first whether existing ones compose. |
| `src/parser/mod.rs` | Parse it. |
| `src/types/checker.rs` | `synth` arm. |
| `src/eval/mod.rs` | `eval` arm. |

**Prefer composing existing variants over adding new ones.** Blocks are the worked
example: `{ a = 1 \n f(a) \n expr }` needs no `Block` node, because the parser
lowers it to nested `Expr::Let` with non-binding statements bound to `_`.

### 4. Verify against the real compiler

```bash
cargo test --quiet                  # Rust suite
tests/check_roc.sh --strict         # all four outputs must match, every pair
```

`--strict` is the bar. To see the square explicitly for one feature:

```bash
F=tests/roc/09_records/field_access
for f in $F.roc $F.desugared.roc; do
  printf '%-42s roc=%-20s int=%s\n' "${f##*/}" \
    "$(roc run "$f" 2>&1)" \
    "$(./target/debug/rocflight "$f" 2>&1 | grep -v '^\[Desugaring\]')"
done
```

All four values must be the same string. A feature that matches on the sugared file
but not the desugared one is **not done** — that asymmetry is a real bug, not a
formatting difference (see the golden-pair rule above for the case that proved it).

Finally, the interpreter's own emitted desugaring must be valid Roc:

```bash
./target/debug/rocflight $F.roc
roc check .rocflight/cache/desugared/tests_roc_09_records_field_access_roc.desugared.roc
```

That closes the loop, and it currently holds for all 18 pairs plus
`hello_world/main.roc`: every desugaring the interpreter emits is Roc the real
compiler accepts.

### 5. Done means

- [ ] Golden pair exists, one feature per pair
- [ ] `roc check` clean on both files
- [ ] **All four outputs identical** — `roc` and `rocflight`, sugared and desugared
- [ ] `tests/check_roc.sh --strict` green for the pair
- [ ] `cargo test --quiet` green
- [ ] The interpreter's emitted `.desugared.roc` passes `roc check`
- [ ] Status row updated in IMPLEMENTATION_PHASES.md — with the measured result

---

## A worked example

The `?` operator, sugared and desugared, both verified compiling and producing `3`:

`tests/roc/17_error_handling/question_operator.roc`

```roc
app [main!] {}

parse_both : Str, Str => Try(I64, [BadNumStr])
parse_both = |a, b| {
    x = I64.from_str(a)?
    y = I64.from_str(b)?
    Ok(x + y)
}

main! : List(Str) => Try({}, [Exit(I8), ..])
main! = |_args| {
    match parse_both("1", "2") {
        Ok(n) => echo!(I64.to_str(n))
        Err(_) => echo!("err")
    }
    Ok({})
}
```

`tests/roc/17_error_handling/question_operator.desugared.roc`

```roc
app [main!] {}

# `?` expands to a match on the Try, propagating Err unchanged.
parse_both : Str, Str => Try(I64, [BadNumStr])
parse_both = |a, b| {
    match I64.from_str(a) {
        Ok(x) =>
            match I64.from_str(b) {
                Ok(y) => Ok(x + y)
                Err(e) => Err(e)
            }
        Err(e) => Err(e)
    }
}

main! : List(Str) => Try({}, [Exit(I8), ..])
main! = |_args| {
    match parse_both("1", "2") {
        Ok(n) => echo!(I64.to_str(n))
        Err(_) => echo!("err")
    }
    Ok({})
}
```

Note the error tag is `BadNumStr` — found by annotating `[InvalidNumStr]` and
reading the type back out of the failure. Guessing would have cost a debugging
session; asking the compiler cost one run.
