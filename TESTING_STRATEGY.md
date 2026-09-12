# Testing Strategy - Phase-by-Phase Validation

**Objective:** Ensure full type correctness and syntax support for Roc interpreter  
**Target:** Support `/home/brian/Code/rocflight/roc-compiler/test/echo/all_syntax_test.roc`  
**Method:** Test files with type verification via roc repl

---

## 📋 Test File Organization

### Location
```
tests/roc/
├── phase1_strings_test.roc          # ✅ COMPLETE
├── phase2_numbers_test.roc          # TODO (not yet created)
├── phase3_lambda_test.roc           # TODO (not yet created)
├── phase4_operators_test.roc        # ✅ CREATED
├── phase5_entry_test.roc            # TODO (not yet created)
├── phase6_number_types_test.roc     # ✅ CREATED
├── phase7_integer_ops_test.roc      # TODO
├── phase8_unary_ops_test.roc        # TODO
├── phase9_records_test.roc          # TODO
├── phase10_tuples_test.roc          # TODO
├── phase11_match_test.roc           # ✅ CREATED
├── phase12_tags_test.roc            # TODO
├── phase13_if_else_test.roc         # TODO
├── phase14_nominal_types_test.roc   # TODO
├── phase15_lists_test.roc           # TODO
├── phase16_loops_test.roc           # TODO
├── phase17_error_handling_test.roc  # ✅ CREATED
├── phase18_generics_test.roc        # TODO
├── phase19_pipelines_test.roc       # TODO
└── phase20_effects_test.roc         # TODO
```

---

## 🎯 Using Roc REPL for Type Verification

### Starting the REPL
```bash
cd /home/brian/Code/rocflight/roc-compiler
./roc repl
```

### Checking Types
```roc
# Simple types
> "hello" : Str
"hello" : Str

> 5 : I64
5 : I64

> Bool.True : Bool
True : Bool

# Collection types
> [1, 2, 3] : List(I64)
[1, 2, 3] : List(I64)

> { x: 5, y: 10 } : { x: I64, y: I64 }
{ x: 5, y: 10 } : { x: I64, y: I64 }

# Function types
> |x| x + 1 : I64 -> I64
<function> : I64 -> I64

# Tag unions
> [Red, Green, Blue]
<tag union> : [Red, Green, Blue]

> Ok(5)
Ok(5) : [Ok(I64), ...]

# Try type
> Try(Ok(5), Err("error"))
Try(Ok(5), Err("error")) : [Ok(I64), Err(Str)]
```

### Verifying Function Signatures
```roc
> number_operators : I64, I64 -> _
> number_operators = |a, b| { sum: a + b, diff: a - b, prod: a * b }

> :type number_operators
number_operators : I64, I64 -> { diff: I64, prod: I64, sum: I64 }
```

---

## 🚀 Phase Implementation Workflow

### Step 1: Type Verification (Before Implementation)

**For each phase:**

1. Open the Roc REPL
2. Test each expression from the phase test file
3. Verify the exact types displayed by REPL
4. Document expected types in test file comments

**Example:**
```bash
$ roc repl
> 2 + 3 * 4
14 : I64

> [1, 2, 3]
[1, 2, 3] : List(I64)

> match 5 { 5 => "Five", _ => "Other" }
"Five" : Str
```

### Step 2: Implement Phase in Interpreter

1. Update AST (src/ast/mod.rs)
2. Update parser (src/parser/mod.rs)
3. Update type checker (src/types/checker.rs)
4. Update evaluator (src/eval/mod.rs)

### Step 3: Test Implementation

```bash
# Run phase-specific test file
roc run tests/roc/phaseN_*_test.roc

# Or with Roc interpreter if partially implemented
./target/release/rocflight tests/roc/phaseN_*_test.roc
```

### Step 4: Verify Against Real Roc

```bash
# Compare output with actual Roc
roc run tests/roc/phaseN_*_test.roc > rocflight_output.txt
./target/release/rocflight tests/roc/phaseN_*_test.roc > interpreter_output.txt
diff rocflight_output.txt interpreter_output.txt
```

---

## ✅ Test File Structure

Each test file follows this pattern:

```roc
# Phase X: [Feature Name]
# Test file for [feature description]
#
# Run with: roc run tests/roc/phaseX_*.roc
#
# Type Verification (from roc repl):
# [expected types from REPL]

main! = |_args| {
    echo!("=== Phase X: [Feature] ===\n")

    # Section 1: Basic feature
    echo!("--- Feature 1 ---\n")
    test1 : Type
    test1 = expression
    echo!("Result: ${display(test1)}\n")
    expect test1 == expected_value

    # Section 2: Edge cases
    echo!("--- Feature 2 ---\n")
    test2 : Type
    test2 = expression
    expect test2 == expected_value

    # Summary
    echo!("\n✅ Phase X: All tests passed!\n")
    Ok({})
}

# Type checks:
# expression1 : Type1
# expression2 : Type2
```

---

## 📊 Type Verification Checklist

For each phase, verify in REPL:

- [ ] Basic literal types (String, I64, Bool, etc.)
- [ ] Collection types (List, tuple, record)
- [ ] Union types (tags, Try/Result)
- [ ] Function types (arrows, generics)
- [ ] Return types of operations
- [ ] Type inference (when no annotation)
- [ ] Mixed type operations (Int + Float)
- [ ] Error cases (what should NOT work)

---

## 🎯 Running Test Suite

### All Tests
```bash
# Run all phase tests
for phase in {1..20}; do
    echo "Running Phase $phase..."
    roc run tests/roc/phase${phase}_*.roc 2>&1 | tail -3
done
```

### Specific Phase
```bash
roc run tests/roc/phase6_number_types_test.roc
roc run tests/roc/phase11_match_test.roc
```

### With Error Checking
```bash
roc run tests/roc/phase4_operators_test.roc && echo "✅ PASSED" || echo "❌ FAILED"
```

---

## 🔍 Type Correctness Verification

### Before Implementation
1. Write test file with expected types (from REPL)
2. Run test file with actual Roc to verify expectations
3. Document expected output

### During Implementation
1. Parse and type-check incrementally
2. Run unit tests frequently
3. Compare output with actual Roc

### After Implementation
1. Run full test file
2. Verify every expression matches REPL type
3. Test edge cases and error conditions

---

## 📝 Example: Phase 6 Number Types

### Step 1: REPL Verification
```
> 5.U8 : U8
5 : U8

> 0xFF : I64
255 : I64

> 42.0 : Dec
42.0 : Dec
```

### Step 2: Test File (phase6_number_types_test.roc)
```roc
test_u8 : U8
test_u8 = 255.U8
echo!("255.U8 = ${Num.to_str(test_u8)}\n")
expect test_u8 == 255.U8

hex : I64
hex = 0xFF
echo!("0xFF (hex) = ${Num.to_str(hex)}\n")
expect hex == 255
```

### Step 3: Implementation
- Add U8, U16, U32, I32, etc. to type system
- Parse type suffixes (5.U8, 5.I32)
- Parse number formats (0xFF, 0o77, 0b1010)
- Implement type checking for each
- Evaluate correctly

### Step 4: Verification
```bash
$ roc run tests/roc/phase6_number_types_test.roc
=== Phase 6: Number Types ===
--- Unsigned Integers ---
255.U8 = 255
...
✅ Phase 6: All number type tests passed!
```

---

## 🚨 Common Type Mistakes

### Mistake 1: Ignoring type annotations
❌ `test = 5`  (inferred I64, but might need U64)  
✅ `test : U64 = 5.U64`  (explicit type)

### Mistake 2: Wrong operator precedence
❌ `2 + 3 * 4` returns 20  (should be 14)  
✅ `2 + 3 * 4` returns 14  (multiplication first)

### Mistake 3: Not handling mixed types
❌ `5 + 2.5` fails  (I64 + F64 invalid)  
✅ `5 + 2.5` returns 7.5  (coerced to F64)

### Mistake 4: Pattern exhaustiveness
❌ `match x { Red => ... }` (doesn't handle Green, Blue)  
✅ `match x { Red => ..., Green => ..., Blue => ... }`

### Mistake 5: Missing error handling
❌ `parse_int("abc")` crashes  (no error type)  
✅ `parse_int("abc") : Try(I64, Str)` (returns Err)

---

## 📋 Phase Testing Roadmap

### Quick Validation (30 min)
- Phase 1-5: Already complete, verify with test files
- Ensures foundation is solid

### Core Features (6-8 hours)
- Phase 6: Number types
- Phase 9: Records
- Phase 11: Pattern matching
- Phase 13: If/else

### Advanced Features (8-10 hours)
- Phase 12: Tag unions
- Phase 14: Nominal types
- Phase 17: Error handling
- Phase 20: Effects

### Complete Implementation (20+ hours total)
All 20 phases with full test coverage

---

## ✨ Best Practices

1. **Type before code** - Use REPL to verify types first
2. **One feature at a time** - Complete one phase fully
3. **Test-driven** - Write tests before implementation
4. **Documentation** - Comment expected types in code
5. **Regression testing** - Run all phases after each change
6. **Edge cases** - Test boundaries and error conditions

---

## 📞 Debugging Failed Tests

### If test fails:
1. Run in REPL to verify expected behavior
2. Check type annotations match REPL output
3. Verify operator precedence
4. Check pattern matching exhaustiveness
5. Test with simpler cases first
6. Compare with actual Roc output line-by-line

### If types mismatch:
1. Run expression in REPL
2. Copy exact type from REPL output
3. Update test file type annotation
4. Verify interpreter produces same type

---

## 🎓 Learning Resources

- Roc documentation: https://www.roc-lang.org/
- REPL tutorial: `roc repl` then `:help`
- Type system guide: See all_syntax_test.roc
- Examples: tests/roc/ directory

---

**Status:** Test framework ready for systematic phase-by-phase validation ✅

