# Roc Interpreter - Implementation Phases (Rebuilt)

**Target:** Support `/home/brian/Code/rocflight/roc-compiler/test/echo/all_syntax_test.roc`  
**Priority:** Type correctness using roc repl verification  
**Date:** 2026-09-12

---

## ✅ Completed Phases

### Phase 1: String Literals & Interpolation (COMPLETE)
**Status:** ✅ 5/5 tests passing

**Implemented:**
- String literals: `"hello"`
- String interpolation: `"Value: ${expr}"`
- Escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`
- Type inference: `Str`

**Test File:** `phase1_strings_test.roc` (created below)

---

### Phase 2: Numbers & Type Literals (COMPLETE)
**Status:** ✅ 19/19 tests passing

**Implemented:**
- Integer literals: `42`, `-3`
- Float literals: `3.14`, `-2.5`
- Numeric type inference: `I64`, `F64`
- Variable identifiers

**Test File:** `phase2_numbers_test.roc` (created below)

---

### Phase 6: Numeric Type Variants & Formats (COMPLETE)
**Status:** ✅ Parser & desugarer working

**Implemented:**
- Hex/octal/binary literals: `0xFF`, `0o77`, `0b1010`
- Type suffixes: `255.U8`, `42.I32`, `3.14.F64`, `42.0.Dec`
- Type annotations: `x : U8` (removed by desugarer)
- Extended Type enum: U8-U128, I8-I128, F32, F64, Dec
- Desugaring pass: Remove type annotations from Roc syntax

**Parsing Verified:**
- `0xFF` → 255
- `0o77` → 63  
- `0b1010` → 10
- `255.U8` → parses, evaluates as 255
- `x : U8` → removed by desugarer

**NOT Yet (Deferred):**
- Type checker: Track numeric type suffixes
- Evaluator: Enforce type constraints
- Mixed type arithmetic: proper coercion rules

**Test File:** `phase6_number_types_test.roc` (created below)

---

### Phase 3: Let Bindings & Lambda Functions (COMPLETE)
**Status:** ✅ 16/16 tests passing

**Implemented:**
- Let bindings: `let x = value in body`
- Lambda expressions: `|x| body`, `|x, y| x + y`
- Closures with environment capture
- Variable scoping
- Function application: `f(x)`, `add(1, 2)`

**Test File:** `phase3_lambda_test.roc` (created below)

---

### Phase 4: Binary Operators (PARTIAL - 36/36 tests)
**Status:** ✅ Arithmetic & Comparison & Logical

**Implemented:**
- Arithmetic: `+`, `-`, `*`, `/`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `&&`, `||`
- Operator precedence: 5 levels
- Mixed int/float coercion

**NOT Yet:**
- Integer division: `//` (truncating division)
- Modulo: `%`
- Bitwise: `&`, `|`, `^`, `<<`, `>>`
- Unary: `-x` (negation), `!x` (logical NOT)
- Boolean keywords: `and`, `or` (vs `&&`, `||`)

**Test File:** `phase4_operators_test.roc` (created below)

---

### Phase 5: App Entry Points (COMPLETE)
**Status:** ✅ 12/12 tests passing

**Implemented:**
- App declaration parsing: `app [main!]`
- Entry point extraction
- Effect syntax: `main!` (marked with `!`)
- Built-in functions: `Num.to_str()`, `Stdout.line()`

**Test File:** `phase5_entry_test.roc` (created below)

---

## 🚀 Remaining Phases (Priority Order)

### Phase 7: Integer Division & Modulo (NEXT - HIGH PRIORITY)
**Target:** Support `//` and `%` operators

**Required Types:**
```roc
a // b : I64       # integer division (truncating)
a % b : I64        # modulo
```

**Effort:** Low (30 minutes)  
**Test File:** `phase7_integer_ops_test.roc` (created below)

---

### Phase 8: Unary Operators & Boolean Keywords (MEDIUM PRIORITY)
**Target:** Support unary operators and boolean keywords

**Required:**
```roc
-x : I64           # negation (unary minus)
!x : Bool          # logical NOT

a and b : Bool     # vs a && b
a or b : Bool      # vs a || b
```

**Effort:** Medium (1-2 hours)  
**Test File:** `phase8_unary_ops_test.roc` (created below)

---

### Phase 9: Records (HIGH PRIORITY)
**Target:** Support record types and field access

**Required Types:**
```roc
{ x: I64, y: I64 } : { x: I64, y: I64 }
rec.x : I64        # field access
{ rec & x: 5 }     # record update
{ x, y } = rec     # destructuring
```

**Effort:** High (3-4 hours)  
**Test File:** `phase9_records_test.roc` (created below)

---

### Phase 10: Tuples (MEDIUM PRIORITY)
**Target:** Support tuple types

**Required:**
```roc
("Roc", 1) : (Str, I64)
tup.0 : Str        # tuple indexing
(x, y) = tup       # destructuring
```

**Effort:** Medium (1-2 hours)  
**Test File:** `phase10_tuples_test.roc` (created below)

---

### Phase 11: Pattern Matching (CRITICAL PRIORITY)
**Target:** Support `match` expressions and patterns

**Required:**
```roc
match value {
    pattern1 => expr1
    pattern2 => expr2
    _ => default
}
```

**Pattern Types:**
- Literal patterns: `5`, `"hello"`
- Variable patterns: `x`, `_` (wildcard)
- List patterns: `[]`, `[x]`, `[x, y]`, `[x, ..]`
- Tag patterns: `Red`, `Foo(x, y)`
- Guard clauses: `pattern if condition`

**Effort:** Very High (4-6 hours)  
**Test File:** `phase11_match_test.roc` (created below)

---

### Phase 12: Tag Unions (HIGH PRIORITY)
**Target:** Support tagged union types

**Required:**
```roc
[Red, Green, Blue] : tag union
[Foo(I64, Str), Bar] : tag union with payload
Red : [Red, Green, Blue]
Foo(42, "hello") : [Foo(I64, Str), Bar]
```

**Effort:** High (3-4 hours)  
**Test File:** `phase12_tags_test.roc` (created below)

---

### Phase 13: If/Else Expressions (MEDIUM PRIORITY)
**Target:** Support conditional expressions

**Required:**
```roc
if condition
    then_expr
else
    else_expr

if cond1
    expr1
else if cond2
    expr2
else
    expr3
```

**Effort:** Medium (1-2 hours)  
**Test File:** `phase13_if_else_test.roc` (created below)

---

### Phase 14: Nominal Types (HIGH PRIORITY)
**Target:** Support opaque and nominal type definitions

**Required:**
```roc
NominalType := { x: I64 }
NominalType.{ x: 42 }

Secret :: { key: Str }.{ ... }  # opaque type
```

**Effort:** High (3-4 hours)  
**Test File:** `phase14_nominal_types_test.roc` (created below)

---

### Phase 15: List & Str Built-ins (MEDIUM PRIORITY)
**Target:** Support list type and common operations

**Required:**
```roc
[1, 2, 3] : List(I64)
List.map : (List(a), (a -> b)) -> List(b)
List.fold : (List(a), b, (b, a -> b)) -> b
Str.concat : (Str, Str) -> Str
```

**Effort:** Medium (2-3 hours)  
**Test File:** `phase15_lists_test.roc` (created below)

---

### Phase 16: For/While Loops (MEDIUM PRIORITY)
**Target:** Support imperative loop constructs

**Required:**
```roc
var $count = 0
for item in list {
    $count = $count + 1
}

while condition {
    $var = $var + 1
}
```

**Effort:** High (3-4 hours)  
**Test File:** `phase16_loops_test.roc` (created below)

---

### Phase 17: Error Handling - Try & ? (HIGH PRIORITY)
**Target:** Support error handling with Try type and `?` operator

**Required:**
```roc
Try(Ok(a), Err(e)) : result type
value? : early return on error
value ?? default : provide default for error
first() ? NoFirstError : wrap error
```

**Effort:** Very High (4-5 hours)  
**Test File:** `phase17_error_handling_test.roc` (created below)

---

### Phase 18: Type Variables & Generics (MEDIUM PRIORITY)
**Target:** Support generic type parameters

**Required:**
```roc
identity : a -> a
identity = |x| x

type_var : List(a) -> List(a)
```

**Effort:** High (3-4 hours)  
**Test File:** `phase18_generics_test.roc` (created below)

---

### Phase 19: Pipelines (LOW PRIORITY)
**Target:** Support pipeline operator

**Required:**
```roc
value |> func        # equivalent to func(value)
value |> f1 |> f2    # chaining
```

**Effort:** Low (30 minutes)  
**Test File:** `phase19_pipelines_test.roc` (created below)

---

### Phase 20: Effect System (CRITICAL)
**Target:** Full effect support

**Required:**
```roc
effect! : Str => {}
echo!("message\n")
```

**Effort:** Very High (5-6 hours)  
**Test File:** `phase20_effects_test.roc` (created below)

---

## 📊 Priority Matrix

| Phase | Priority | Effort | Est. Time | Why |
|-------|----------|--------|-----------|-----|
| 6 | HIGH | Medium | 2-3h | Need all numeric types |
| 7 | MEDIUM | Low | 30m | Used in examples |
| 8 | MEDIUM | Medium | 1-2h | Common syntax |
| 9 | HIGH | High | 3-4h | Used everywhere |
| 10 | MEDIUM | Medium | 1-2h | Basic type |
| 11 | CRITICAL | V.High | 4-6h | Pattern matching essential |
| 12 | HIGH | High | 3-4h | Tag unions everywhere |
| 13 | MEDIUM | Medium | 1-2h | Conditional logic |
| 14 | HIGH | High | 3-4h | Opaque types |
| 15 | MEDIUM | Medium | 2-3h | List operations |
| 16 | MEDIUM | High | 3-4h | Loop constructs |
| 17 | HIGH | V.High | 4-5h | Error handling |
| 18 | MEDIUM | High | 3-4h | Generics needed |
| 19 | LOW | Low | 30m | Syntactic sugar |
| 20 | CRITICAL | V.High | 5-6h | Effects core to Roc |

---

## 🎯 Recommended Implementation Order

### Sprint 1: Arithmetic Foundation (HIGH)
1. Phase 6: Number types (2-3h)
2. Phase 7: Integer division (30m)
3. Phase 8: Unary & boolean keywords (1-2h)

### Sprint 2: Data Structures (HIGH)
4. Phase 10: Tuples (1-2h)
5. Phase 9: Records (3-4h)
6. Phase 12: Tag unions (3-4h)

### Sprint 3: Control Flow (HIGH)
7. Phase 13: If/else (1-2h)
8. Phase 11: Pattern matching (4-6h)

### Sprint 4: Built-ins & Collections (MEDIUM)
9. Phase 15: Lists & Str (2-3h)
10. Phase 19: Pipelines (30m)

### Sprint 5: Advanced Features (HIGH)
11. Phase 14: Nominal types (3-4h)
12. Phase 16: Loops (3-4h)
13. Phase 18: Generics (3-4h)

### Sprint 6: Professional Features (CRITICAL)
14. Phase 17: Error handling (4-5h)
15. Phase 20: Effects (5-6h)

---

## ⚠️ Key Challenges

### Type System
- Full Hindley-Milner with constraints
- Type variables and generic parameters
- Effect system (unique to Roc)
- Nominal vs structural typing

### Parser
- Whitespace-sensitive (indentation matters)
- Multiple syntaxes for same construct
- Guard clauses in patterns
- Destructuring patterns

### Evaluation
- Pattern matching semantics
- Error propagation with `?`
- Mutable variables with `var`
- Effect system runtime

---

## 🔍 Using Roc REPL for Type Verification

**Command to check types:**
```bash
roc repl
```

**Example session:**
```roc
> 5 : I64
5 : I64

> "hello" : Str
"hello" : Str

> [1, 2, 3] : List(I64)
[1, 2, 3] : List(I64)

> { x: 1, y: 2 } : { x: I64, y: I64 }
{ x: 1, y: 2 } : { x: I64, y: I64 }

> \x -> x + 1 : I64 -> I64
<function> : I64 -> I64
```

---

## 📝 Each Phase Has a Test File

Each phase will have:
1. **Type verification** - Using roc repl output
2. **Syntax examples** - All valid constructs
3. **Test expressions** - Correct output to console
4. **Error cases** - What should fail and why

**After implementing each phase, run:**
```bash
roc run phase_N_test.roc
```

---

## 🚀 Next Step

Create comprehensive test files for each phase (starting below).

