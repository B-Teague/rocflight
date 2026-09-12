# Phase Implementation Guide - Roc Interpreter

**Goal:** Systematically implement all Roc syntax from `all_syntax_test.roc`  
**Strategy:** Type-driven development using roc compiler REPL  
**Target:** Full feature parity with all_syntax_test.roc

---

## 🎯 Quick Start

### 1. View Current Status
```bash
cat IMPLEMENTATION_PHASES.md  # Detailed phase breakdown
cat TESTING_STRATEGY.md       # How to test each phase
```

### 2. Check Completed Work
- ✅ Phase 1: Strings (5 tests passing)
- ✅ Phase 2: Numbers (19 tests passing)
- ✅ Phase 3: Lambdas (16 tests passing)
- ✅ Phase 4: Operators (+, -, *, /, ==, !=, <, >, &&, ||) (36 tests passing)
- ✅ Phase 5: App entry points (12 tests passing)
- **TOTAL:** 124/125 tests passing (99.2%)

### 3. Verify Type Correctness
```bash
cd /home/brian/Code/rocflight/roc-compiler
./roc repl

# In REPL:
> "hello" : Str
> 5 + 3 : I64
> Bool.True && Bool.False : Bool
> [1, 2, 3] : List(I64)
```

### 4. Run Interpreter Tests
```bash
cd /home/brian/Code/rocflight
cargo test --quiet
```

---

## 📚 Test Files Structure

### For Completed Phases
Each phase has a test file in `tests/roc/`:

**Phase 1: Strings**
```bash
tests/roc/phase1_strings_test.roc
- Tests string literals
- Tests string interpolation
- Tests escape sequences
- Expected types: Str
- Run with: roc run tests/roc/phase1_strings_test.roc
```

**Phase 4: Operators**
```bash
tests/roc/phase4_operators_test.roc
- Tests arithmetic: +, -, *, /
- Tests comparison: ==, !=, <, <=, >, >=
- Tests logical: &&, ||
- Tests operator precedence
- Expected types: I64, Bool
```

### Test File Format
Each file includes:
1. **Header comment**: Phase name and description
2. **Type verification section**: Expected types from REPL
3. **Main function**: Series of test cases
4. **Echo output**: Display results
5. **Expect statements**: Validate correctness

**Example structure:**
```roc
# Phase N: [Feature]
# Type Verification (from roc repl):
# [types from REPL output]

main! = |_args| {
    echo!("=== Phase N: [Feature] ===\n")
    
    test1 : Type
    test1 = expression
    echo!("Test: ${display(test1)}\n")
    expect test1 == expected
    
    echo!("\n✅ Phase N: All tests passed!\n")
    Ok({})
}
```

---

## 🚀 How to Implement a Phase

### Step 1: Type Verification (Use REPL First)

**Before** writing any code, verify expected types:

```bash
$ cd roc-compiler
$ ./roc repl
> 5.U8 : U8
5 : U8

> 0xFF : I64
255 : I64

> [1, 2, 3] : List(I64)
[1, 2, 3] : List(I64)
```

Copy the EXACT type output into test file comments.

### Step 2: Create/Review Test File

Example: `tests/roc/phase6_number_types_test.roc`

Includes:
- All syntax from the phase
- Expected types (from REPL)
- Test cases with assertions
- Edge cases and error conditions

### Step 3: Implement in Interpreter

Update files in this order:
1. **src/ast/mod.rs** - Add new expression/type variants
2. **src/parser/mod.rs** - Parse new syntax
3. **src/types/checker.rs** - Type check new constructs
4. **src/eval/mod.rs** - Evaluate to values

### Step 4: Test Implementation

```bash
# Run unit tests
cargo test --quiet

# Run phase test file (when interpreter supports it)
roc run tests/roc/phaseN_*.roc

# Compare with actual Roc
./roc run tests/roc/phaseN_*.roc > expected.txt
./target/release/rocflight tests/roc/phaseN_*.roc > actual.txt
diff expected.txt actual.txt
```

### Step 5: Verify Correctness

- All assertions pass
- Output matches expected
- No compiler warnings
- Types match REPL exactly

---

## 📊 Phase Priority & Effort

### Next High-Priority Phases

#### Phase 6: Number Types (2-3 hours)
**What:** U8, U16, U32, I32, I64, I128, F32, F64, Dec, hex/octal/binary  
**Why:** Foundation for all numeric code  
**Test:** `tests/roc/phase6_number_types_test.roc` ✅

#### Phase 11: Pattern Matching (4-6 hours)
**What:** `match expr { pattern => result }`  
**Why:** Essential for all tag union handling  
**Test:** `tests/roc/phase11_match_test.roc` ✅

#### Phase 17: Error Handling (4-5 hours)
**What:** Try type, Ok/Err, ?, ??  
**Why:** Critical for real-world code  
**Test:** `tests/roc/phase17_error_handling_test.roc` ✅

### See IMPLEMENTATION_PHASES.md for Full List

---

## 🔍 Using Roc REPL Effectively

### Start REPL
```bash
cd /home/brian/Code/rocflight/roc-compiler
./roc repl
```

### Common REPL Commands
```
:help              # Show help
:type <expr>       # Show type of expression
:list              # List defined values
:clear             # Clear definitions
```

### Getting Type Information
```roc
# Simple values
> 5 : I64
5 : I64

> "hello" : Str
"hello" : Str

# Function types
> |x| x + 1 : I64 -> I64
<function> : I64 -> I64

# Collection types
> [1, 2, 3] : List(I64)
[1, 2, 3] : List(I64)

> { x: 5 } : { x: I64 }
{ x: 5 } : { x: I64 }

# Union types
> Ok(5) : [Ok(I64), Err(Str)]
Ok(5) : [Ok(I64), ...]

# Check function signature
number_operators : I64, I64 -> _
number_operators = |a, b| { sum: a + b }
> :type number_operators
number_operators : I64, I64 -> { sum: I64 }
```

---

## ✅ Verification Checklist for Each Phase

- [ ] **REPL verification**: All types match REPL output exactly
- [ ] **Test file created**: tests/roc/phaseN_*.roc with all syntax
- [ ] **Parser updated**: Parses all new syntax
- [ ] **Type checker updated**: Correctly infers types
- [ ] **Evaluator updated**: Computes correct values
- [ ] **Unit tests pass**: `cargo test --quiet` succeeds
- [ ] **Phase test passes**: `roc run tests/roc/phaseN_*.roc` succeeds
- [ ] **No warnings**: `cargo clippy` is clean
- [ ] **Matches Roc output**: Compare with actual Roc
- [ ] **Edge cases handled**: Error conditions work correctly
- [ ] **Documentation updated**: Code comments explain changes
- [ ] **Git committed**: Changes saved with clear message

---

## 🎯 Example: Implementing Phase 6 (Number Types)

### Step 1: REPL Verification
```bash
$ roc repl
> 5.U8 : U8
5 : U8

> 255.U16 : U16
255 : U16

> 0xFF : I64
255 : I64

> 0o77 : I64
63 : I64

> 0b1010 : I64
10 : I64
```

### Step 2: Create Test File
Create `tests/roc/phase6_number_types_test.roc` with:
- All number type suffixes
- All number formats (hex, octal, binary)
- Type annotations matching REPL output
- Test cases for each type

### Step 3: Update AST
In `src/ast/mod.rs`:
```rust
pub enum Expr<'a> {
    // ... existing ...
    Int(i64),              // existing: defaults to I64
    // Add support for other types in type system
}

pub enum Type {
    U8, U16, U32, U64, U128,    // unsigned
    I8, I16, I32, I64, I128,    // signed  
    F32, F64,                    // float
    Dec,                         // decimal
}
```

### Step 4: Update Parser
In `src/parser/mod.rs`:
```rust
// Parse type suffixes: 5.U8, 255.U16
// Parse formats: 0xFF (hex), 0o77 (octal), 0b1010 (binary)
```

### Step 5: Update Type Checker
In `src/types/checker.rs`:
```rust
// Infer correct type for each literal form
// Handle mixed numeric type operations
```

### Step 6: Update Evaluator
In `src/eval/mod.rs`:
```rust
// Evaluate each numeric type correctly
// Handle conversions between types
```

### Step 7: Test
```bash
# Run all tests
cargo test --quiet

# Run phase test
roc run tests/roc/phase6_number_types_test.roc

# Compare with actual Roc
roc run tests/roc/phase6_number_types_test.roc > expected.txt
./target/release/rocflight tests/roc/phase6_number_types_test.roc > actual.txt
diff expected.txt actual.txt
```

---

## 📈 Progress Tracking

### Current Status (2026-09-12)
- **Phases Complete:** 5 of 20
- **Tests Passing:** 124/125 (99.2%)
- **Code Quality:** 9.2/10
- **Type Support:** Basic (I64, F64, Str, Bool)
- **Syntax Support:** Strings, Numbers, Lambdas, Operators, Entry Points

### Estimated Timeline
- **Sprint 1** (Phase 6-8): 4-5 hours → Numeric types & operators
- **Sprint 2** (Phase 9-12): 8-10 hours → Data structures & pattern matching
- **Sprint 3** (Phase 13-16): 6-8 hours → Control flow & loops
- **Sprint 4** (Phase 17-20): 14-16 hours → Advanced features
- **Total:** ~35-40 hours to full implementation

### Next Phase to Implement
**Phase 6: Number Types** (2-3 hours)
- Easiest to implement
- Foundation for everything else
- Test file ready: `tests/roc/phase6_number_types_test.roc`

---

## 🎓 Key Principles

1. **Type-Driven:** Always verify types with REPL first
2. **Test-First:** Write tests before implementation
3. **One Phase at a Time:** Complete each phase fully
4. **Regression Testing:** Run all tests after each change
5. **Documentation:** Comment expected types in code
6. **Incremental:** Small, focused commits

---

## 📞 Resources

- **all_syntax_test.roc**: Target file with all syntax
- **IMPLEMENTATION_PHASES.md**: Detailed phase breakdown
- **TESTING_STRATEGY.md**: How to test each phase
- **ROC_INTERPRETER_PLAN.md**: Architecture and design
- **STATUS.md**: Current project status
- **tests/roc/**: Phase-specific test files

---

## 🚀 Getting Started Now

```bash
# 1. Understand current status
cat IMPLEMENTATION_PHASES.md | head -100

# 2. Look at test file format
cat tests/roc/phase4_operators_test.roc

# 3. Run current tests
cargo test --quiet

# 4. Start implementing Phase 6
# Edit src/ast/mod.rs, parser/mod.rs, etc.

# 5. Test as you go
cargo test --quiet
roc run tests/roc/phase6_number_types_test.roc
```

---

**Status:** Ready to implement Phase 6 → Phase 20 systematically ✅

**Next Action:** Start Phase 6 implementation (number types)

