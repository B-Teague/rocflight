# Roc Desugaring Rules

**Purpose:** Convert Roc shorthand syntax into explicit, verbose functional forms before parsing.

This keeps the parser simple and makes the AST clean and unambiguous.

---

## Desugaring Pipeline

Every Roc source file undergoes desugaring in this order:

1. **Type Annotations** — *preserved verbatim* (Rule 1). The parser skips them.
2. **Effect Type Arrows** — `=>` *left alone* (Rule 2). It is never `->`.
3. **Error Propagation** (`?`) and **Default Values** (`??`) — done in the PARSER,
   not here. `??` needs its operand's extent; `?` has to move the rest of the block
   into the `Ok` arm, which raw-text substitution cannot locate reliably. See
   `Parser::propagate_error`.
4. **Optional Field Access** (`.?`) — implemented in the parser as `Expr::OptionalField`;
   yields `Ok(value)` or `Err(MissingField)`. It is NOT sugar for a match: the presence
   test happens at run time.
5. **Optional Record Fields** (`?:`) — implemented as `Type::Optional` on a nominal's
   backing record. A record without the field still unifies.

**Result:** valid Roc with its types explicit — code that passes `roc check` on its
own, not merely something the parser can read. Passes 1 and 2 are deliberate
no-ops; they are listed because both used to transform and both were wrong.

---

## Rule 1: Type Annotations — PRESERVED, not removed

Roc allows annotations on their own line:
```roc
x : I64
x = 42
```

**Desugaring: none. The annotation is kept verbatim.**

The desugared file is a real Roc program that must pass `roc check` on its own, with
its types explicit — that is the whole reason the file exists (see
PHASE_IMPLEMENTATION_GUIDE.md, "The golden-pair rule"). Deleting annotations made
the emitted file un-compilable and threw away exactly the type information the
desugared form is supposed to state.

**Skipping annotations is the parser's job**, not the desugarer's:
`Parser::skip_type_annotation`.

### What this reversed

Pass 0 used to delete `identifier : Type` whenever `identifier = value` followed. It
is now a documented no-op. Two things learned when it changed:

- An annotation is `name : Type` — whitespace before the colon. A record field is
  `name: value`. The parser needs that distinction or it eats record fields.
- A missed annotation line **truncates the top-level binding chain**: the binding
  after it never gets parsed. This showed up as `main!` being an "Undefined
  variable" whenever anything annotated sat above it. `Parser::skip_trivia` handles
  whitespace, comments and annotations together so there is one place to get right.

---

## Rule 2: Effect Type Arrows — NOT desugared

`=>` is the effectful-function arrow:
```roc
main! : List(Str) => Try({}, [Exit(I8), ..])
```

**Desugaring: none. `=>` is never rewritten to `->`.**

This rule previously said to replace `=>` with `->` "because the parser only
understands `->`". That is backwards: `=>` carries the effectfulness, and `->` means
a pure function. Rewriting it discards the distinction the annotation exists to
make, and contradicts Rule 7 below.

`=>` also appears as the **arm separator in `match`**, so a blind text replacement
corrupts every match expression in the file:

```roc
match color {
    Red => "red"        # not a function type
}
```

The parser skips annotations entirely (Rule 1), so it never needs to understand
either arrow in a type position.

**Note the entry-point type**: the default host requires
`List(Str) => Try(_a, [Exit(I8), ..])` — `Exit(I8)`, not `Exit(I32)`, and `Try`, not
`Result` (`Result` is not in scope at all). Verified with `roc check` on
nightly-2026-09-03.

---

## Rule 3: Error Propagation with `?`

**Problem:** The `?` operator is shorthand for error propagation:
```roc
result = risky_operation()?
x = result + 5
```

Means: "If `risky_operation()` returns `Err(e)`, propagate that error. Otherwise extract the `Ok(v)`."

**Desugaring:** Expand to explicit match expression:
```roc
result = match risky_operation() {
    Ok(v) => v
    Err(e) => return Err(e)
}
x = result + 5
```

**Why:** Match expressions are the fundamental construct. This makes error handling explicit.

**Pattern:** `expr?` becomes:
```roc
match expr {
    Ok(v) => v
    Err(e) => return Err(e)
}
```

**Rules:**
- Only apply after complete expressions (not mid-expression)
- Must be postfix operator (comes after the expression)
- Works on any `Try(T, E)` type
- Early returns propagate the error to the enclosing function

**Example:**
```roc
# Before:
first = strings.first()?
num = I64.from_str(first)?

# After:
first = match strings.first() {
    Ok(v) => v
    Err(e) => return Err(e)
}
num = match I64.from_str(first) {
    Ok(v) => v
    Err(e) => return Err(e)
}
```

---

## Rule 4: Default Values with `??`

**Problem:** The `??` operator provides a default on error:
```roc
count = I64.from_str(input) ?? 0
```

Means: "If parsing succeeds, use the result. If it fails, use 0."

**Desugaring:** Expand to match expression:
```roc
count = match I64.from_str(input) {
    Ok(v) => v
    Err(_) => 0
}
```

**Why:** Match expressions are explicit and clear about both branches.

**Pattern:** `expr ?? default` becomes:
```roc
match expr {
    Ok(v) => v
    Err(_) => default
}
```

**Rules:**
- Only for `Try(T, E)` types
- Error is ignored (discarded with `_`)
- Default value can be any expression

**Example:**
```roc
# Before:
timeout = Config.get_timeout() ?? 5000

# After:
timeout = match Config.get_timeout() {
    Ok(v) => v
    Err(_) => 5000
}
```

---

## Rule 5: Optional Field Access with `.?`

**Problem:** Optional record fields return `Try(value, MissingField)`:
```roc
config : ServerConfig
result = config.?timeout_ms
```

Means: "Get `timeout_ms` field if it exists, wrapped in a Try."

**Desugaring:** Convert to function call that produces Try:
```roc
config : ServerConfig
result = Query.optional(config, "timeout_ms")  # or internal representation
```

**Why:** Field access is syntactic sugar for a runtime check.

**Pattern:** `record.?field` becomes:
```roc
Query.optional(record, @tag("field"))
```

Or more likely represented as a built-in operation in the AST.

**Rules:**
- Only on record types with optional fields (marked `field ?: Type`)
- Returns `Try(T, MissingField)` where T is the field type
- Must be matched or unwrapped

**Example:**
```roc
# Before:
timeout = config.?timeout_ms
result = match timeout {
    Ok(ms) => ms
    Err(MissingField) => "none"
}

# After (after desugaring .? and matching):
timeout = Query.optional(config, @tag("timeout_ms"))
result = match timeout {
    Ok(ms) => ms
    Err(MissingField) => "none"
}
```

---

## Rule 6: Optional Record Fields

**Problem:** Records can have optional fields marked with `?:`:
```roc
Config := {
    host : Str,
    port : U16 ?? 8080,        # defaulted field
    timeout ?: U64              # optional field
}
```

**Desugaring:** Mark in type/record for field access:
```roc
Config := {
    host : Str,
    port : U16,                 # store default elsewhere
    timeout : U64?              # mark as optional
}
```

**Why:** Type system needs to know which fields are optional for `.?` access.

**Pattern:** `field ?: Type` marks field as optional in the record.

**Rules:**
- Only in record type definitions
- Optional fields must be queried with `.?field`
- Required fields can use `.field`

---

## Rule 7: Effectful Function Marker (!) — NOT DESUGARING

**Critical:** the `!` suffix **IS part of the function name.** It is not an
operator and not sugar. There is nothing to desugar.

Upstream (`roc-compiler/src/parse/tokenize.zig`, `chompIdentGeneral`) chomps
`!` straight into the identifier — `echo!` is a single `LowerIdent` token.
Naming an effectful binding without `!` is only a warning
(`roc-compiler/src/check/problem/types.zig`, `EffectfulFunctionName`).

```roc
# Before and after desugaring — identical:
echo!("hello")
main! = |_args| { ... }
```

Do **not** strip the `!`, and do **not** wrap `!` calls in error handling.
`!` says nothing about whether a call returns `Try`; effectfulness lives in the
type (`=>`), and error propagation is the `?` operator's job (Rule 1).

### Related things that are also not this

| Syntax | What it is | Handled where |
|---|---|---|
| `!foo` | unary logical not | upstream canonicalizes to a `Bool.not` call (`roc-compiler/src/base/mod.zig`, `CalledVia.unary_op`) |
| `a != b` | not-equals operator | its own token (`OpNotEquals`) |
| `=>` in an annotation | effectful function type | the type checker (`fn_effectful`); it is **not** `->` |
| `=>` in a `match` arm | arm separator | the parser; also **not** `->` |

### Regression this replaces

An earlier version of Pass 2 scanned for `!` character by character and emitted
Rust into `.roc` output:

```roc
# What it produced for `echo!("hello")` — not Roc:
match echo("hello") { Ok(v) => v, Err(e) => return Err(e) }
```

Roc's match arms are newline-separated, `echo!` has no `Ok`/`Err` to match on,
and the same pass collapsed `a != b` into `a = b`, deleted unary `!` (inverting
the logic), and rewrote every `match` arm's `=>` to `->`. When desugaring a
construct, check what the Zig compiler actually does with it first — the target
is always Roc source or a Roc builtin.

---

## Example: Full Desugaring

**Input:**
```roc
parse_config : Str => Try(Config, ParseErr)
parse_config = |input| {
    host = input.?host ??  "localhost"
    port = Str.to_u16(input.?port)?
    
    Ok({ host, port })
}
```

**After Desugaring:**
```roc
parse_config : Str => Try(Config, ParseErr)   # `=>` is unchanged
parse_config = |input| {
    host = match input.?host {
        Ok(v) => v
        Err(_) => "localhost"
    }
    port = match match input.?port {
        Ok(v) => v
        Err(_) => "default_port"
    } {
        Ok(v) => Str.to_u16(v)
        Err(e) => return Err(e)
    }
    
    Ok({ host, port })
}
```

Wait, this is getting nested and complex. The actual desugaring needs careful sequencing.

---

## Implementation Notes

### Pass 2 (removed)

There is no effect-arrow or `!`-wrapping pass. `!` is part of the identifier and
`=>` is meaningful where it appears, so both pass through untouched. See Rule 7.

### Remaining Passes

3. Expand `?` and `??` (requires careful position detection)
4. Handle `.?` field access (requires expression parsing)
5. Mark optional fields (requires record parsing)

### String Matching Pitfalls

- Don't process inside string literals: `"hello ? world"` shouldn't match
- Don't process inside comments: `# this ? is fine`
- Don't process `???` or `??!` (only exact patterns)
- Handle escaped characters in strings: `"quote: \""`

### Error Handling

Every desugaring step must:
- Return `ParseError` on syntax issues
- Preserve line numbers and positions for error reporting
- Never silently skip or modify code

---

## Testing Strategy

Each desugaring rule must have tests showing:
1. Input shorthand
2. Expected desugared output
3. Edge cases (multiple occurrences, nesting, etc.)
4. Interaction with other rules

Example test — note the Roc arm syntax: newline-separated, no commas:
```
Input: "x = risky()?"
Expected:
    x = match risky() {
        Ok(v) => v
        Err(e) => return Err(e)
    }
```

Every expected output must be valid Roc. If an expectation would not parse with
the compiler in `roc-compiler/`, the expectation is the bug.

---

## Current Status

| Rule | Status | Implementation |
|------|--------|-----------------|
| 1. Type Annotations | ✅ Done | `remove_type_annotations()` |
| 2. Effect Arrows + ! Wrapping | ⛔ Removed | Was never desugaring — see Rule 7 |
| 3. Error Propagation `?` | ❌ Placeholder | Needs full implementation |
| 4. Default Values `??` | ❌ Placeholder | Needs full implementation |
| 5. Optional Field Access `.?` | ❌ Placeholder | Needs full implementation |
| 6. Optional Record Fields | ❌ Placeholder | Needs full implementation |
| 7. Effectful Marker `!` | ⛔ Not desugaring | `!` is part of the identifier; pass it through |

---

## Next Steps

1. ✅ Document all desugaring rules (this file)
2. Implement `?` operator expansion to match expressions
3. Implement `??` operator expansion to match expressions
4. Implement `.?` field access handling
5. Write comprehensive tests for each rule
6. Verify desugared output matches Roc semantics
