# Exact Target: /home/brian/Code/rocflight/hello_world/main.roc

## The Exact File We're Building Toward

```roc
app [main!] { pf: platform "https://github.com/roc-lang/basic-cli/releases/download/0.20.0/X73hGh05nNTkDHU06FHC0YfFaQB1pimX7gncRcao5mU.tar.br" }

import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```

**Output should be:**
```
There are -3 birds.
```

---

## Line-by-Line Breakdown

### Line 1: App Declaration
```roc
app [main!] { pf: platform "..." }
```
**What this does:** Declares the app, exports `main!`, loads platform `pf`

**Type verification with roc repl:**
```
> app [main!] { pf: platform "..." }
```

**Implementation phases needed:**
- Parser: handle `app`, `[exports]`, `{ name: platform "url" }`
- Type system: module metadata
- **Phase:** Phase 16+ (Module/Import system - defer to v2.0)

**For now:** Skip or mock the app declaration, focus on the executable code

---

### Line 3: Import Statement
```roc
import pf.Stdout
```
**What this does:** Imports `Stdout` module from platform `pf`

**Type verification with roc repl:**
```
> import pf.Stdout
> Stdout.line! : Str => {}
```

**Implementation phases needed:**
- Parser: `import Module.Name`
- Type system: Module resolution
- **Phase:** Phase 16+ (Module/Import system - defer to v2.0)

**For now:** Hardcode `Stdout.line!` as builtin

---

### Line 5: Variable Binding with Negative Number
```roc
birds = -3
```
**Type:** `I64`
**Value:** `-3`

**Type verification with roc repl:**
```
> birds : I64
> birds = -3
```

**Implementation phases needed:**
- ✅ Phase 1: String literals (foundation)
- → **Phase 2:** Integer literals (positive and negative)
- → **Phase 3:** Variable binding and lookup

**Testing:**
```bash
roc repl
> birds = -3
> birds
-3
```

---

### Line 7: Effectful Function Definition
```roc
main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```

**Type:** `(List Str) => {}`  
**Meaning:** Function takes `List Str`, produces effects (outputs text), returns `{}`

**Type verification with roc repl:**
```
> main! : (List Str) => {}
> main! = |_args| 
>   Stdout.line!("There are ${Num.to_str(3)} birds.")
```

**Implementation phases needed:**
- → **Phase 7:** Lambda expressions (`|_args| body`)
  - Parameter syntax
  - Underscore (unused variable)
  - Multi-line body with indentation
- → **Phase X:** Effects (effectful functions with `!`)
  - Function names ending with `!`
  - Effect type notation `=>` 
  - **Status:** Planned for v2.0 (not in Phase 1-15)

**For v1.0:** Treat all functions as effectful (no distinction)

---

### Line 8: Effectful Function Call with String Interpolation
```roc
Stdout.line!("There are ${Num.to_str(birds)} birds.")
```

**Breaks down into:**

#### Part A: Function call with qualifier
```roc
Stdout.line!(arg)
```
**Type:** `Str => {}`  
**Meaning:** Takes String, produces effect (outputs to stdout)

**Type verification with roc repl:**
```
> Stdout.line! : Str => {}
> Stdout.line!("test")
```

**Implementation phases needed:**
- → **Phase 8:** Function calls `func(args)`
- → **Phase 15:** Builtin functions with qualifiers `Module.func`
  - Need to implement: `Stdout.line! : Str => {}`

---

#### Part B: String interpolation with nested function call
```roc
"There are ${Num.to_str(birds)} birds."
```

**Type:** `Str`

**Type verification with roc repl:**
```
> birds : I64
> birds = -3
> Num.to_str(birds) : Str
> "There are ${Num.to_str(birds)} birds." : Str
```

**Implementation phases needed:**
- ✅ **Phase 1:** String literals and escape sequences
- → **Phase 14:** String interpolation `${expr}`
  - Expression evaluation inside `${...}`
  - Type checking inside interpolation
  - Conversion to string via `to_str()`
- → **Phase 15:** Builtin function `Num.to_str`
  - Need: `Num.to_str : I64 -> Str`

---

## Implementation Roadmap (Ordered by Dependency)

### Checkpoints

**✅ Checkpoint 1: Phase 1 (COMPLETE)**
```roc
"Hello, world!"
```
✅ Working: `cargo run -- examples/simple_hello.roc`

---

**→ Checkpoint 2: Phase 1 + 2 + 3**
```roc
birds = -3
```
- Parse negative integer
- Infer type as `I64`
- Store in variable
- Look up variable value
- **Test file:** `examples/checkpoint2.roc`

---

**→ Checkpoint 3: Phase 1-3 + 14 + 15**
```roc
birds = -3
"There are ${Num.to_str(birds)} birds."
```
- Interpolation in string
- Call builtin function inside interpolation
- Type-check function call
- Convert result to string
- **Test file:** `examples/checkpoint3.roc`

---

**→ Checkpoint 4: Phase 1-8 + 14-15**
```roc
birds = -3
func = |_args| "There are ${Num.to_str(birds)} birds."
func([])
```
- Define lambda with parameter
- Call lambda with arguments
- Capture variable from outer scope (closure)
- **Test file:** `examples/checkpoint4.roc`

---

**→ Checkpoint 5: Full hello_world (Requires Effects - v2.0)**
```roc
app [main!] { pf: platform "..." }
import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```
- App declaration (skip for v1.0, hardcode)
- Imports (skip for v1.0, hardcode Stdout)
- Variable binding
- Effectful function
- Effectful function call
- String interpolation
- **Status:** Requires Phase X (Effects support, v2.0)

---

## Phase Implementation Order

| Phase | Feature | For hello_world | Status |
|-------|---------|-----------------|--------|
| 1 | Strings | ✅ Used in interpolation | ✅ Complete |
| 2 | Numbers | ✅ `birds = -3` | → Next |
| 3 | Variables | ✅ `birds` binding/lookup | → Phase 3 |
| 4 | Operators | ✅ Arithmetic (optional) | → Phase 4+ |
| 7 | Lambdas | ✅ `\|_args\|` syntax | → Phase 7 |
| 8 | Calls | ✅ `Num.to_str(birds)` | → Phase 8 |
| 14 | Interpolation | ✅ `"${expr}"` | → Phase 14 |
| 15 | Builtins | ✅ `Num.to_str`, `Stdout.line!` | → Phase 15 |
| X | Effects | ✅ `main!`, `=>` notation | → v2.0 |
| 16+ | Imports/App | Optional for v1.0 | → v2.0 |

---

## Simplified Milestones (Skip Effects for v1.0)

For v1.0, we'll mock effects and imports. Here's the actual runnable progression:

### Milestone 1: Variable with negative number
```roc
birds = -3
birds
```
**Needs:** Phase 2 (numbers), Phase 3 (variables)
**Expected output:** `-3` with type `I64`

### Milestone 2: String with interpolation
```roc
birds = -3
"There are ${Num.to_str(birds)} birds."
```
**Needs:** Phase 2, 3, 14 (interpolation), 15 (Num.to_str)
**Expected output:** `"There are -3 birds."` with type `Str`

### Milestone 3: Lambda returning string
```roc
birds = -3
main = |_args|
  "There are ${Num.to_str(birds)} birds."

main([])
```
**Needs:** Phase 2, 3, 7 (lambdas), 8 (calls), 14, 15
**Expected output:** `"There are -3 birds."` with type `Str`

### Milestone 4: Simulate hello_world (mock effects)
```roc
# For v1.0, treat as non-effectful
birds = -3
main = |_args|
  "There are ${Num.to_str(birds)} birds."

main([])
```
**Status:** Works after Phase 15

### Milestone 5: Full hello_world (with effects - v2.0)
```roc
app [main!] { pf: platform "..." }
import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```
**Status:** Requires Phase X (effects) and Phase 16+ (imports)

---

## Type Verification Checklist (Using roc repl)

Before implementing each phase, verify:

**Phase 2: Numbers**
```bash
$ roc repl
> birds : I64
> birds = -3
> birds
-3
> Num.to_str(birds)
```

**Phase 3: Variables**
```bash
> x : I64
> x = 5
> y : I64
> y = x
> y
5
```

**Phase 7: Lambdas**
```bash
> f : List(Str) -> Str
> f = |_args| "test"
> f([])
"test"
```

**Phase 14: Interpolation**
```bash
> birds = -3
> "There are ${Num.to_str(birds)} birds."
"There are -3 birds."
```

**Phase 15: Builtins**
```bash
> Num.to_str(-3)
"-3"
> Stdout.line!("hello")
hello
```

---

## Testing Each Checkpoint

Create test files in `examples/`:

```bash
# After Phase 2-3
cargo run -- examples/checkpoint2.roc
# Expected: Type: I64, Result: -3

# After Phase 2-3, 14-15
cargo run -- examples/checkpoint3.roc
# Expected: Type: Str, Result: "There are -3 birds."

# After Phase 2-3, 7-8, 14-15
cargo run -- examples/checkpoint4.roc
# Expected: Type: Str, Result: "There are -3 birds."

# After Phase 2-3, 7-8, 14-15 (mock effects)
cargo run -- examples/checkpoint5.roc
# Expected: Print "There are -3 birds."
```

---

## Summary

**Target:** Run exactly `/home/brian/Code/rocflight/hello_world/main.roc`

**Realistic v1.0 goal:** Run equivalent code without effects/imports
```roc
birds = -3
result = "There are ${Num.to_str(birds)} birds."
result
```

**Full hello_world:** Requires effects support (Phase X, v2.0)

**Next implementation:** Phase 2 (Integer & Float Literals)
