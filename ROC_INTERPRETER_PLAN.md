# Roc Interpreter Implementation Plan (v3)

Architecture and design. The feature list is in
[IMPLEMENTATION_PHASES.md](IMPLEMENTATION_PHASES.md); the per-feature workflow is in
[PHASE_IMPLEMENTATION_GUIDE.md](PHASE_IMPLEMENTATION_GUIDE.md).

**Verified against:** `roc` nightly-2026-09-03-62fcb65. Facts here were checked by
running the compiler and its LSP, not recalled.

---

## Ground truth comes from the compiler, per feature

Before implementing anything, pin the real types. Never from memory — Roc's type
system is subtle in exactly the places that matter (`Dec` vs `I64`, effect arrows,
nominal `Try`), and a wrong assumption costs a rewrite of the parser arm that
encodes it.

Two tools, in order of usefulness:

**1. The LSP** — exact types for a real file, which is how `echo!`'s signature was
established:

```bash
roc experimental-lsp --stdio        # drive over JSON-RPC
```

`textDocument/hover` gives the type at a position, `textDocument/completion` gives
a `detail` per symbol, `textDocument/inlayHint` annotates a whole file. It reports
`hoverProvider`, `definitionProvider`, `inlayHintProvider`, `completionProvider`
among its capabilities.

**2. Deliberately wrong annotations.** `roc check` names the type it expected,
which is faster than guessing:

```bash
printf 'app [main!] {}\n\nmain! : Str => Str\nmain! = |_| ""\n' > /tmp/t.roc
roc check /tmp/t.roc
#   But the platform requires:
#       List(Str) => Try(_a, [Exit(I8), ..])
```

`roc repl` is fine for scratch work, but it is not the reference: it cannot show
you what a *platform* requires of an entry point, and that is where the
interesting constraints live.

**Reference file for all syntax:** `roc-compiler/test/echo/all_syntax_test.roc`.

---

## Goal: run hello_world/main.roc

```roc
app [main!] {}

main! : List(Str) => Try({}, [Exit(I8), ..])
main! = |_args| {
    echo!("hello world\n")
    Ok({})
}
```

**Status: working.** `rocflight hello_world/main.roc` and `roc run
hello_world/main.roc` both print `hello world`, and the desugaring the interpreter
produces for it passes `roc check` and runs to the same output. That round trip — sugared in, valid annotated Roc out,
same answer from both — is the property the whole test suite is built on.

### The entry-point model, corrected

This plan previously assumed hello_world was a **basic-cli app**: a real platform
URL, `import pf.Stdout`, `Stdout.line!(...)`. That is a valid shape, but it is not
this target's shape, and building the interpreter around it put the platform
downloader on the critical path for printing a string.

What is actually true:

- A **platformless app** — no platform in the header — is what `roc run` links the
  built-in default host for. `app [main!] {}`, or no header at all.
- **The default host provides `echo!`, not the compiler.** Signature `Str => {}`
  per LSP hover. It is not a global builtin: in a type module it fails with
  "Nothing is named echo! in this scope."
- The default host demands exactly `main! : List(Str) => Try(_a, [Exit(I8), ..])`.
  `Try`, not `Result` — `Result` is not in scope at all.
- `echo!` writes with **no trailing newline**.

So the interpreter models host effects in a small table,
[`src/platform/host.rs`](src/platform/host.rs), kept separate from compiler
builtins because the language draws that line too. Real platform loading becomes a
later phase that *replaces* the table, rather than a prerequisite for phase 1.

`main!` itself is current, ordinary syntax — it is the export name for effectful
apps with real platforms too, appearing that way ~435 times in the compiler's own
test suite. Nothing about it is legacy.

### What the default host is not

It is not a stand-in for a real platform, and the two differ in ways that matter
once phase 19 lands:

| | Default host | Real platform |
|---|---|---|
| Header | `app [main!] {}` | `app [main!] { pf: platform "..." }` |
| Effects | `echo!` only | Whatever the platform's `hosted` block declares |
| Entry type | `List(Str) => Try(_a, [Exit(I8), ..])` | The platform's own requirement |
| Export name | `main!` | Platform's choice — `main`, `process_string`, a record of functions |

`roc-compiler/test/str/app.roc` exports a plain `process_string : Str -> Str` with
no `main` at all; `test/int/app.roc` exports `main` as a *record* of
`init`/`update`/`render`. Do not hardcode `main!` as the universal entry point.

---

## Platform loading

**Implemented** — `src/platform/resolve.rs` and `src/platform/real.rs`.

The design decision that shaped it: **`roc` already downloads, verifies and extracts
dependencies**, so none of that is reimplemented. A dependency URL ends in
`<HASH>.tar.zst`, and `roc` extracts it to `~/.cache/roc/packages/<HASH>/`. Mapping a
URL to its sources is therefore string manipulation, not networking. `roc` is the
authority on cache layout, hashing and archive integrity; duplicating it would mean a
second implementation to keep in step for no gain.

An earlier draft of this plan described downloading a *brotli*-compressed tar and
untarring it. That was Rust-era: the format is `.tar.zst` now, and the cache is
content-addressed rather than mirroring the URL path. Both are recognised.

What is read from a platform's sources:

| From | What |
|---|---|
| `main.roc` | `requires` entry-point type, `exposes` module list, `hosted` symbol names |
| `<Module>.roc` | members declared inside `Name :: [].{ ... }`, with signatures |

**The limit is architectural, not an omission.** A platform's `hosted` functions are
implemented in its compiled host — native code. An interpreter has nothing
to call. So effects run only where this interpreter supplies its own implementation,
and anything else the platform declares is reported as a gap that names which effects
*are* available. `HOST_EFFECTS` in `platform/host.rs` still covers the default
platformless host (`echo!`); a real platform's declarations are read from its sources.

**Ceiling:** packages (`alias: "URL"` without `platform`) are resolved but not
evaluated — their contents are ordinary Roc the interpreter would have to run.
**Upgrade path:** evaluate package modules; implement more effects natively.

## Architecture overview

### Type System Strategy
- **Hindley-Milner inference** integrated into parser AST construction (not a separate pass)
- **Unification during evaluation** - constraints solved on first use (simpler, faster for v1.0)
- **Type annotations optional** - inferred types verified at runtime boundaries
- **Error messages** include inferred type + expected type + source location

### Memory Strategy
- **Arena allocator** - use `bumpalo` crate (proven, minimal complexity)
- **String interning** - via `once_cell::sync::Lazy<StringPool>` + `String::leak`
- **Value variants** - use `Box` only for recursive structures; flat for primitives
- **Stack-based environment** - scope markers (no Vec<HashMap>)

### Why This Works
- `bumpalo` is battle-tested; gives arena semantics without reimplementation
- String interning is automatic via leaked references—no reference counting overhead
- Unification-during-evaluation avoids separate type-check phase
- Caching (.rocache/) is optional Phase 16; Phase 1-15 focus on correctness

---

## Type system core

### Type Representation
```
Type ::=
  | I64 | I32 | I16 | I8 | U64 | U32 | U16 | U8 | I128 | U128
  | F64 | F32 | Dec              (number types)
  | Bool | Str                   (primitives)
  | List(Box<Type>)              (parametric)
  | Record { fields: Map<String, Type> }
  | TagUnion { tags: Map<String, Vec<Type>> }  ([Ok(a), Err(b)])
  | Function(Box<Type>, Box<Type>)  (a -> b; right-associative)
  | TypeVar(u32)                 (unbound: $0, $1, ...)
  | Nominal(String, Vec<Type>)   (NominalTypeRecord, Try(a, b))
  | Unit                         ({} — the empty record)
```

This is the target. What `src/types/mod.rs` has **today** is the numeric types,
`Str`, `Bool`, `TypeVar`, `List`, `Function` and `Unit`; `Record`, `TagUnion` and
`Nominal` arrive with phases 09, 12 and 14. Tag expressions currently type as a
fresh var, so `Try` is unmodelled — which is why phase 17's golden pair passes under
`roc` but not yet under the interpreter.

### Bidirectional Type Checking
- **Synthesis**: `synth(expr) -> Type` — infer type from expression structure
- **Checking**: `check(expr, expected) -> Result<(), TypeError>` — verify expr matches type

Example flow for `|a, b| a + b`:
```
1. Parse → Lambda { params: [a, b], body: BinOp(...) }
2. Synth at call site → (TypeVar($0), TypeVar($1)) -> TypeVar($2)
3. At application: check args → unify($0, I64), unify($1, I64)
4. Unify body result → unify($2, I64)
5. Final: (I64, I64) -> I64
```

### Unification Algorithm (During Evaluation)
```
unify(t1: Type, t2: Type) -> Result<Substitution, UnifyError> {
  if t1 == t2: return Ok(∅)
  if t1 = TypeVar(v1) and occurs_check(v1, t2): unify_var(v1, t2)
  if t2 = TypeVar(v2) and occurs_check(v2, t1): unify_var(v2, t1)
  if t1 = List(a) and t2 = List(b): unify(a, b)
  if t1 = (a1 -> b1) and t2 = (a2 -> b2):
    unify(a1, a2) ++ unify(b1, b2)
  else: Err(UnifyError::Mismatch(t1, t2))
}
```

### Constraint Storage & Solving
- **TypeVar bindings**: `HashMap<u32, Type>` (substitution table)
- **Constraints**: collected during parsing, solved incrementally
- **Occurs check**: prevent infinite types (`t = List(t)`)
- **Error context**: track source location + constraint chain for hints

---

## Memory model

### Value Layout
```rust
// Flat enum: most values fit in 16 bytes (tag + data)
pub enum Value {
    // Immediates (8 bytes)
    Bool(bool),           // 1 byte + 7 padding
    I64(i64),
    F64(f64),
    
    // References (pointer + metadata)
    Str(&'static str),    // interned, lifetime: 'static
    List(Box<Vec<Value>>), // heap-allocated list
    Record(Box<HashMap<&'static str, Value>>), // interned keys
    Tag {
        name: &'static str,  // interned
        payload: Box<Vec<Value>>,
    },
    Closure {
        params: Box<Vec<&'static str>>, // interned names
        body: Box<Expr>,
        env: Box<Environment>,
    },
}
```

### String Interning Pool
```rust
lazy_static::lazy_static! {
    static ref STRING_POOL: Mutex<StringPool> = Mutex::new(StringPool::new());
}

pub struct StringPool {
    strings: HashSet<&'static str>,
}

pub fn intern(s: &str) -> &'static str {
    let mut pool = STRING_POOL.lock().unwrap();
    pool.get_or_insert(s)
}
```

**Ceiling**: Single global lock. Upgrade to per-thread pools if parsing contention matters.

### Arena Allocator (AST Nodes)
```rust
pub struct AstArena {
    arena: bumpalo::Bump,
}

impl AstArena {
    pub fn new() -> Self { ... }
    pub fn alloc<T>(&self, val: T) -> &'a T { self.arena.alloc(val) }
}

// All Expr pointers use arena lifetimes:
pub enum Expr<'a> {
    BinOp(&'a Expr<'a>, Op, &'a Expr<'a>),
    If { cond: &'a Expr<'a>, ... },
    // No Box<Expr> — &'a Expr is smaller
}
```

**Ceiling**: Entire AST lives in one arena per file; freed when parsing completes. Upgrade to persistent AST cache if Phase 16+ caching needed.

### Stack-Based Environment (No HashMap per scope)
```rust
pub struct Environment {
    stack: Vec<StackFrame>,
}

pub struct StackFrame {
    vars: Vec<(usize, &'static str, Value)>, // (scope_depth, name, value)
    scope_depth: usize,
}

pub fn lookup(&self, name: &str) -> Result<Value, EvalError> {
    self.stack.iter().rev()
        .find_map(|frame| frame.vars.iter()
            .rfind(|(_, n, _)| *n == name)
            .map(|(_, _, v)| v.clone()))
        .ok_or(...)
}
```

**Ceiling**: O(n) lookup per scope depth. Upgrade to flat index map if lookup is hot.

---

## Phase roadmap

Lives in **[IMPLEMENTATION_PHASES.md](IMPLEMENTATION_PHASES.md)**, with measured
per-feature status.

It used to be duplicated here as a 15-phase list, which drifted out of sync with
the 20-phase list in that file until the two disagreed about both numbering and
what was finished. One list, one place.

What belongs here instead is the shape every phase shares:

| Layer | File | Rule |
|---|---|---|
| AST | `src/ast/mod.rs` | Add a variant only if existing ones cannot compose. Blocks needed none — the parser lowers them to nested `Expr::Let`. |
| Parser | `src/parser/mod.rs` | Skip type annotations (`skip_type_annotation`); never delete them from the source. |
| Types | `src/types/checker.rs` | A `synth` arm. Unknown constructs get a fresh var, not a guess. |
| Eval | `src/eval/mod.rs` | An `eval` arm. |

And the gate every phase passes: a golden pair per syntax feature, both files
compiling under `roc check`, identical output from both, explicit types in the
desugared one. See the testing section below.

## Desugaring pass (before AST construction)

Sugar is expanded to explicit syntax before the parser runs, so the parser has no
special cases and the AST stays small.

```
.roc → Desugarer → desugared .roc → Parser → AST → Type Checker → Evaluator
                        │
                        └── must pass `roc check` on its own
```

The desugared source being **real, compilable Roc with explicit types** is the point,
not a debugging nicety: it is what lets every desugaring be checked by the actual
compiler instead of trusted. A debug build prints it with `--show-desugared`; nothing
is written to disk.

### What is sugar

| Syntax | Example | Desugars to | Phase |
|---|---|---|---|
| `?` | `f(x)?` | `match f(x) { Ok(v) => v, Err(e) => Err(e) }` | 17 |
| `??` | `expr ?? d` | `match expr { Ok(v) => v, Err(_) => d }` | 17 |
| `.?` | `rec.?field` | Try-producing field access | 09 |
| `?:` | `field ?: Type` | Optional record field | 09 |
| implicit precedence | `2 + 3 * 4` | `2 + (3 * 4)` | 04 |
| type suffix | `255.U8` | `small : U8` + `small = 255` | 02 |

### What is NOT sugar

Each of these was previously listed in this document as a desugaring. All four were
wrong, and two of them produced broken output before being removed:

| Syntax | Previously claimed | Actually |
|---|---|---|
| `foo!` | strip `!` from the name | The `!` is **part of the identifier**. `echo` and `echo!` are different names; LSP completion returns the literal label `echo!`. Nothing to rewrite. |
| `=>` | rewrite to `->` | The effectful-function arrow, and the `match` arm separator. Never `->`. |
| `!foo` | — | Unary logical not; canonicalises to `Bool.not(foo)`. Unrelated to the `!` above. |
| `"${x}"` | — | A primitive string form, not sugar for concatenation. |

The old effect pass also emitted Rust (`Err(e) => return Err(e)`) into what was
supposed to be a Roc file — which is exactly the class of bug that requiring the
output to pass `roc check` catches immediately.

### Type annotations are preserved

`x : Type` lines survive desugaring untouched. The parser skips them
(`Parser::skip_type_annotation`); the desugarer does not delete them.

The earlier design had the desugarer strip annotations so the parser never saw
them. That made the emitted file un-compilable as Roc and discarded the very types
the desugared form is meant to state explicitly. Two consequences worth knowing:

- An annotation is `name : Type` (whitespace before the colon); a record field is
  `name: value`. The parser relies on that distinction, so a record literal is not
  mistaken for an annotation.
- A missed annotation line silently **truncates the top-level binding chain**, so
  the binding after it is never parsed. That was a real bug: `main!` came back as
  "Undefined variable" whenever anything was annotated above it. `skip_trivia`
  handles whitespace, comments and annotations in one place for this reason.

**Reference:** [DESUGARING.md](DESUGARING.md) for the per-rule detail.
**Source of truth:** `roc-compiler/test/echo/all_syntax_test.roc`, verified with
`roc check`.

---

## Type system specification

### Unification Algorithm (Complete)

```rust
pub fn unify(t1: &Type, t2: &Type, subst: &mut Substitution) -> Result<(), TypeError> {
    let t1 = deref(t1, subst);
    let t2 = deref(t2, subst);

    if t1 == t2 { return Ok(()); }

    match (&t1, &t2) {
        (Type::TypeVar(v1), Type::TypeVar(v2)) if v1 == v2 => Ok(()),
        (Type::TypeVar(v), t) | (t, Type::TypeVar(v)) => {
            if occurs_check(v, t, subst) {
                Err(TypeError::InfiniteType(*v, t.clone()))
            } else {
                subst.insert(*v, t.clone());
                Ok(())
            }
        }
        (Type::List(a), Type::List(b)) => unify(a, b, subst),
        (Type::Record(f1), Type::Record(f2)) => {
            if f1.len() != f2.len() { return Err(TypeError::RecordMismatch); }
            for (k, t1) in f1 {
                if let Some(t2) = f2.get(k) {
                    unify(t1, t2, subst)?;
                } else {
                    return Err(TypeError::MissingField(k.clone()));
                }
            }
            Ok(())
        }
        (Type::Func(a1, b1), Type::Func(a2, b2)) => {
            unify(a1, a2, subst)?;
            unify(b1, b2, subst)
        }
        _ => Err(TypeError::Mismatch(Box::new(t1), Box::new(t2))),
    }
}

fn deref(t: &Type, subst: &Substitution) -> Type {
    if let Type::TypeVar(v) = t {
        if let Some(t2) = subst.get(v) {
            return deref(t2, subst);
        }
    }
    t.clone()
}
```

### Error Messages with Locations

```rust
pub struct TypeError {
    pub message: String,
    pub expected: Type,
    pub actual: Type,
    pub location: SourceLocation,
    pub context: Vec<String>,
}

impl Display for TypeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{} Type Error\n", self.location.line, self.location.col)?;
        write!(f, "  Expected: {}\n", self.expected)?;
        write!(f, "  Actual:   {}\n", self.actual)?;
        for hint in &self.context {
            write!(f, "  Note: {}\n", hint)?;
        }
        Ok(())
    }
}
```

---

## Testing strategy

**One golden pair of `.roc` files per syntax feature**, and **all four outputs must
be byte-identical**:

```
        roc run <sugared>   ═══   roc run <desugared>
              ║                          ║
     rocflight <sugared>   ═══   rocflight <desugared>
```

```
tests/roc/<NN>_<phase>/<syntax>.roc              # sugared
tests/roc/<NN>_<phase>/<syntax>.desugared.roc    # explicit types, no sugar
```

Requirements, all enforced by `tests/check_roc.sh`:

1. `roc check` clean on **both** files.
2. All four outputs above identical. The horizontal edge proves the desugaring
   preserves semantics; the vertical edges hold the interpreter to the real compiler
   on **both** forms.
3. The desugared file carries **explicit top-level annotations**.
4. **One feature per pair.** `+` and `//` get separate files.

Two gates, so a correctly-written pair is not confused with a finished feature:

| Gate | Means | Fatal? |
|---|---|---|
| **PAIR** | Both files check; the two `roc run` outputs agree; annotations present. Failing this means *the test files are wrong*. | Always |
| **INTERP** | `rocflight` matches `roc` on **both** files. Failing this means *the feature is not implemented yet*. | Only under `--strict` |

```bash
tests/check_roc.sh                 # PAIR fatal, INTERP reported as PEND
tests/check_roc.sh --strict        # both fatal — the definition of done
cargo test --quiet                 # Rust side
```

### Checking the desugared file with the interpreter is not redundant

It is a **different path through the parser**, and it caught a bug the sugared files
could not: top-level bindings parsed their value with `parse_call_expr` rather than
the full precedence chain, so `a = 2 + (3 * 4)` failed at the top level while the
same binding inside a block worked. The sugared pairs kept their bindings inside
`main!`'s block; the desugared ones lift them to the top level with annotations.
That asymmetry is exactly what the fourth output tests.

Expect this to keep happening. The desugared form exercises explicit annotations,
top-level bindings and parenthesised structure — all code paths the sugared form can
skip entirely.

### Tests that touch the global platform cache

`PLATFORM_CACHE` is a process-global `Mutex<HashMap>`, and `clear_cache()` wipes
every entry. Cargo runs a binary's tests on parallel threads, so a test that clears
the cache lands in the middle of another test's assertions — this made
`test_multiple_platforms_in_cache` fail about one run in three.

**Any test that reads or writes the global cache must take the cache test lock
first** (`cache::lock_for_test()` in the lib, `lock_cache()` in
`tests/phase1b_platform_test.rs` — separate processes, so each has its own). Tests
using `PlatformLoader::load()` do not cache and need no lock.

Both locks recover from poisoning (`unwrap_or_else(|e| e.into_inner())`). Without
that, a test panicking while holding the lock makes every later cache test fail with
`PoisonError`, hiding which one actually broke.

### Holding the interpreter to itself

The desugaring the interpreter *emits* must also be valid Roc:

```bash
./target/debug/rocflight --show-desugared hello_world/main.roc 2>&1 \
  | sed -n '/=== DESUGARED CODE ===/,/=== END DESUGARED CODE ===/p' \
  | sed '1d;$d' > /tmp/desugared.roc
roc check /tmp/desugared.roc
```

Currently true for all 18 pairs plus `hello_world/main.roc`.

A feature is done when `--strict` is green for its pair and the status row in
IMPLEMENTATION_PHASES.md reflects the measured result rather than the intended one.

### Why the desugared file must compile

If the desugarer's output is not valid Roc, it cannot be diffed against the real
compiler, and a desugaring bug hides until it surfaces as a wrong answer with no
obvious cause. Making the output compilable turns every desugaring into something
`roc` itself will check.

This is why the desugarer **preserves** type annotations. It used to strip them so
the parser never had to skip them — which made the emitted file un-compilable and
discarded exactly the type information the desugared form exists to make explicit.
Skipping is the parser's job.

## File layout

As it actually is. `find src tests -type f` is the authority; this table says what
each file is for.

```
src/
  main.rs                 CLI: <file.roc>, test, version, help
  lib.rs                  exports
  error.rs                ParseError and friends

  desugaring/mod.rs       sugar expansion; PRESERVES type annotations
  parser/mod.rs           parse_expr; skip_type_annotation; parse_block
  ast/mod.rs              Expr, BinOp, StrPart
  types/
    mod.rs                Type enum
    checker.rs            synth / unify
  eval/
    mod.rs                eval loop, call_builtin, call_host_effect
    value.rs              Value enum
    environment.rs        stack-based scopes
  platform/
    host.rs               default-host effect table (echo!)
    loader.rs             platform download/parse — mock for now
    module.rs, cache.rs, mod.rs
  memory/string_pool.rs   interning

tests/
  check_roc.sh            golden-pair gate: both files check, outputs agree
  roc/<NN>_<phase>/       golden pairs, one per syntax feature
    <syntax>.roc
    <syntax>.desugared.roc
  entry_point_test.rs     the platformless entry-point model
  phase*_test.rs          per-phase Rust tests
```

Not present, despite earlier drafts of this plan listing them: `parser/lexer.rs`,
`ast/display.rs`, `types/substitution.rs`, `types/error.rs`, `eval/builtins.rs`,
`memory/arena.rs`. Some may arrive with the phases that need them; none should be
created speculatively.

**Dependencies:** see `Cargo.toml`. The plan previously named nom, bumpalo,
once_cell and regex as given; check before assuming any of them is in use.

---

## Key architectural decisions

| Decision | Why | Ceiling | Upgrade Path |
|----------|-----|---------|--------------|
| Unification during eval, not before | Simpler: constraints solved on first use; fewer passes | Type errors only at runtime in some cases | Phase 20: Add compile-time type-check pass before eval |
| Single global StringPool with mutex | Proven crate (once_cell), no custom interning | Single-threaded contention; 1-2% overhead | Per-thread pools if concurrency needed |
| Arena for AST, not persistent | All Expr lifetimes tied to arena; freed after eval | No AST reuse across files in v1.0 | Phase 16: Cache ASTs, use stable addresses |
| No custom memory pools, use bumpalo | Fewer lines of code; battle-tested | No fine-grained allocation control | Phase 25: Custom allocators per AST size class |
| Stack-based env, O(n) lookup | Linear per scope depth; simple to implement | 5+ scope nesting slows slightly | Phase 20: Flat index map if profiling shows hot |
| ~~No bytecode, tree-walk only~~ **superseded** | Simpler eval loop; matches interpreter size budget | ~10-20% slower than bytecode | Done: a register VM replaced the tree-walker, 2-8x faster. See OPTIMIZATION_PLAN.md |
| Type errors include inferred + expected | More helpful debug; no extra cost | Longer error messages | (no upgrade needed) |
| Builtin functions as native Rust | Fastest path; no need for VM | Can't define builtins in Roc | Phase 25: Host-language FFI, user-defined builtins |

---

## Performance targets

Aspirational, not measured. Treat as budgets to check against, not as results.

| Metric | Target | Ceiling |
|--------|--------|---------|
| Parse + Type Check | <100ms for 1000-line file | Single-threaded; no parallelism in v1.0 |
| Runtime eval | <10ms for recursive fib(20) | Met on the register VM |
| Memory per file | <10MB for 1000-line program | Arena + strings; no cleanup until end |
| Startup | <50ms cold | No caching; Phase 16 helps |
| String pool size | <1MB for typical program | Interning; no dedup of semantically-equal strings |

---

## Next steps

In order, cheapest-unblocking first. Full list with measured status in
[IMPLEMENTATION_PHASES.md](IMPLEMENTATION_PHASES.md).

1. **Nested calls as arguments.** The parser rejects `I64.to_str(inc(41))` — a call
   used as an argument to another call. Three golden pairs are blocked on this one
   fix and nothing needs to precede it.
2. **`//` and `%`.** Same shape as the existing binops.
3. **Records** (phase 09). Every later phase's tests want them for grouping results,
   and two operator pairs are already written against them.
4. **`if`/`else`** (06), then **tag unions** (12), then **`match`** (11). `match` is
   untestable without tags; anything returning `Try` needs both.
5. **`?` and the rest of the error sugar** (17). Its golden pair is written and
   green under `roc` already, so the target is unambiguous.

Each one starts by writing the golden pair, not by editing Rust — if the pair will
not compile, the feature is not yet understood well enough to implement.

---

## Status

Measured per-feature status lives in [IMPLEMENTATION_PHASES.md](IMPLEMENTATION_PHASES.md); project status in [STATUS.md](STATUS.md).

A hand-maintained progress log used to sit here, carrying test counts and a self-assigned code-quality score. Both went stale, and neither was reproducible from the repo. Numbers in this project should come from `cargo test` and `tests/check_roc.sh`.
