# Roc Desugaring Rules

**Purpose:** Convert Roc shorthand syntax into explicit, verbose functional forms before parsing.

This keeps the parser simple and makes the AST clean and unambiguous.

---

## Desugaring Pipeline

Every Roc source file undergoes desugaring in this order:

1. **Type Annotations** — Remove type annotations on separate lines
2. **Effect Type Arrows** — Convert `=>` to `->` in type signatures
3. **Error Propagation** — Expand `expr?` to match expression
4. **Default Values** — Expand `expr ?? default` to match expression  
5. **Optional Field Access** — Expand `.?field` to Try-producing call
6. **Optional Record Fields** — Mark fields with `?:` as optional

**Result:** Clean, verbose, unambiguous code ready for the parser.

---

## Rule 1: Type Annotations

**Problem:** Roc allows type annotations on separate lines from bindings:
```roc
x : I64
x = 42
```

**Desugaring:** Remove the type annotation line, keep only the binding:
```roc
x = 42
```

**Why:** The type checker will infer the type. Annotations are metadata, not needed for AST construction.

**Implementation:**
- Remove lines matching pattern: `identifier : Type` when followed by `identifier = value`
- Keep the binding line

---

## Rule 2: Effect Type Arrows

**Problem:** Effect types use `=>` to indicate "returns an Effect":
```roc
main! : List(Str) => Try({}, [Exit(I32)])
```

**Desugaring:** Convert `=>` to `->` (effect info is in the return type):
```roc
main! : List(Str) -> Try({}, [Exit(I32)])
```

**Why:** The parser only understands `->` for function types. The `Try` type itself marks it as effectful.

**Implementation:**
- Find `=>` outside strings
- Replace with `->`
- Only in type contexts (between `:` and `=`)

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

## Rule 7: Effectful Function Marker (!) - REMOVED

**Critical:** The `!` suffix is **NOT** part of the function name - it's a postfix operator!

**What it means:** Functions marked with `!` return a `Try/Result` type (they're effectful).

**Desugaring:** Remove the `!` from all function names and calls:
```roc
# Before:
echo!("hello")
main! = |_args| { ... }

# After (! removed):
echo("hello")
main = |_args| { ... }
```

**Why:** The `!` is syntactic sugar that marks a function as performing effects. The actual function name doesn't include it. Full error handling wrapping happens in Pass 4.

**Example:**
```roc
# Before desugaring:
result = echo!("message")

# After full desugaring (Pass 4 adds error wrapping):
result = match echo("message") {
    Ok(v) => v
    Err(e) => return Err(e)
}
```

The `!` tells the parser "this call returns Try, wrap it in error handling," but it's not part of the identifier itself.

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
parse_config : Str -> Try(Config, ParseErr)
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

### Parsing Order

1. Parse type annotations first (simplest)
2. Convert `=>` to `->` (simple string replacement)
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

Example test:
```
Input: "x = risky()?"
Expected: "x = match risky() { Ok(v) => v, Err(e) => return Err(e) }"
```

---

## Current Status

| Rule | Status | Implementation |
|------|--------|-----------------|
| 1. Type Annotations | ✅ Done | `remove_type_annotations()` |
| 2. Effect Arrows | ✅ Done | `desugar_effects()` converts `=>` to `->` |
| 3. Error Propagation `?` | ❌ Placeholder | Needs full implementation |
| 4. Default Values `??` | ❌ Placeholder | Needs full implementation |
| 5. Optional Field Access `.?` | ❌ Placeholder | Needs full implementation |
| 6. Optional Record Fields | ❌ Placeholder | Needs full implementation |
| 7. Effectful Names `!` | ✅ Done | Preserved as identifier suffix |

---

## Next Steps

1. ✅ Document all desugaring rules (this file)
2. Implement `?` operator expansion to match expressions
3. Implement `??` operator expansion to match expressions
4. Implement `.?` field access handling
5. Write comprehensive tests for each rule
6. Verify desugared output matches Roc semantics
