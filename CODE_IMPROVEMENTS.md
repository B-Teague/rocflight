# Code Improvements Action Plan

**Priority:** Fix critical issues first, then high-priority improvements

---

## PHASE 1: CRITICAL FIXES (Must Do)

### Fix 1: Enable Type Checking (30 min)
**File:** src/main.rs  
**Current:** Type checking disabled, errors silently ignored  
**Fix:** Actually check types and fail if there are errors

```diff
- let _type_result = type_checker.synth(&ast);
- // Note: Type result not printed - only actual program output is shown
+ match type_checker.synth(&ast) {
+     Ok(_ty) => {
+         // Type check passed - continue
+     }
+     Err(e) => {
+         eprintln!("{}", e);
+         process::exit(1);
+     }
+ }
```

**Impact:** Catches real Roc type errors, matches Roc behavior

---

### Fix 2: Fix Compiler Warnings (15 min)
**Files:** src/types/checker.rs, src/desugaring/mod.rs  
**Current:** 4 compiler warnings about unused variables

```diff
- pub struct ParseError { module, name } => {
+ pub struct ParseError { module: _, name: _ } => {

- pub fn save_debug(&self, original_path: &str, desugared: &str) -> ... {
+ pub fn save_debug(&self, _original_path: &str, _desugared: &str) -> ... {
```

**Impact:** Clean build output, follows Rust idioms

---

### Fix 3: Validate App Entry Point (20 min)
**File:** src/main.rs  
**Current:** String matching is fragile, silently fails

```rust
if !entry_name.ends_with('!') {
    eprintln!("Invalid app entry point: {} (must end with '!')", entry_name);
    process::exit(1);
}
let lookup_name = &entry_name[..entry_name.len() - 1];
```

**Impact:** Clear error messages, prevents silent failures

---

## PHASE 2: HIGH PRIORITY (Should Do Soon)

### Fix 4: Use thiserror Crate (1 hour)
**File:** src/error.rs  
**Why:** Already in Cargo.toml but not used  
**Benefit:** Better error messages, cleaner code

```rust
use thiserror::Error;

#[derive(Error, Debug, Clone)]
#[error("Parse error at position {position}: {message}")]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

#[derive(Error, Debug, Clone)]
#[error("Type error at {line}:{col}\n  Expected: {expected}\n  Actual: {actual}\n  {message}")]
pub struct TypeError {
    pub message: String,
    pub expected: String,
    pub actual: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Error, Debug, Clone)]
#[error("Runtime error: {message}")]
pub struct EvalError {
    pub message: String,
}
```

**Impact:** Cleaner error types, proper Error trait

---

### Fix 5: Refactor main.rs with Result (45 min)
**File:** src/main.rs  
**Current:** Verbose match statements throughout  
**Goal:** Use `?` operator for clean error handling

```rust
use std::error::Error;

fn run(filename: &str) -> Result<(), Box<dyn Error>> {
    let (ast, app_entry_point) = Parser::from_file(filename)?;
    
    let mut type_checker = TypeChecker::new();
    type_checker.synth(&ast)?;
    
    let mut evaluator = Evaluator::new();
    let _value = evaluator.eval(&ast)?;
    
    if let Some(entry_name) = app_entry_point {
        invoke_app_entry_point(&mut evaluator, &entry_name)?;
    }
    
    Ok(())
}

fn main() {
    if let Err(e) = run(&std::env::args().nth(1).unwrap_or_default()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn invoke_app_entry_point(evaluator: &mut Evaluator, entry_name: &str) 
    -> Result<(), Box<dyn Error>> {
    // Validation
    if !entry_name.ends_with('!') {
        return Err(format!("Invalid entry point: {}", entry_name).into());
    }
    
    let lookup_name = &entry_name[..entry_name.len() - 1];
    
    // Lookup and invoke
    let entry_fn = evaluator.env.lookup(lookup_name)
        .ok_or_else(|| format!("App entry point '{}' not found", entry_name))?;
    
    // ... rest of implementation
    Ok(())
}
```

**Impact:** More readable, proper error propagation

---

### Fix 6: Add Error Location Tracking (1.5 hours)
**File:** src/error.rs, src/parser/mod.rs  
**Current:** ParseError only has position, not line/column  
**Fix:** Track line/column for better error messages

```rust
#[derive(Error, Debug, Clone)]
#[error("Parse error at {line}:{column}: {message}")]
pub struct ParseError {
    pub message: String,
    pub position: usize,
    pub line: usize,
    pub column: usize,
}

impl ParseError {
    pub fn new(message: &str, position: usize, input: &str) -> Self {
        let (line, column) = Self::compute_position(position, input);
        ParseError {
            message: message.to_string(),
            position,
            line,
            column,
        }
    }
    
    fn compute_position(position: usize, input: &str) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;
        for (i, ch) in input.chars().enumerate() {
            if i >= position { break; }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }
}
```

**Impact:** Better error messages for multi-line files

---

## PHASE 3: MEDIUM PRIORITY (Nice to Have)

### Fix 7: Remove Unnecessary String Clone (5 min)
**File:** src/parser/mod.rs:61

```diff
- let rest = &self.input[self.pos..].to_string();
+ let rest = &self.input[self.pos..];
```

**Impact:** Slight performance improvement, idiomatic Rust

---

### Fix 8: Add Default for Evaluator (5 min)
**File:** src/eval/mod.rs

```rust
impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
```

**Impact:** More idiomatic, enables `..Default::default()` patterns

---

### Fix 9: Encapsulate Parser State (30 min)
**File:** src/parser/mod.rs

```rust
struct Parser {
    input: String,
    pos: usize,
    app_entry_point: Option<String>,  // Make private
}

impl Parser {
    pub fn app_entry_point(&self) -> Option<&str> {
        self.app_entry_point.as_deref()
    }
}
```

**Impact:** Better encapsulation, prevents accidental state mutation

---

## PHASE 4: ADVANCED (Consider for Later)

### Fix 10: Replace Unsafe Transmute with Rc
**File:** src/eval/mod.rs, src/ast/mod.rs  
**Complexity:** High (requires AST changes)  
**Benefit:** Eliminate unsafe code

```rust
use std::rc::Rc;

pub enum Expr<'a> {
    Lambda {
        params: Vec<&'static str>,
        body: Rc<Expr<'a>>,  // Use Rc instead of Box
    },
}
```

**Note:** This changes AST structure - only do if refactoring

---

### Fix 11: Optimize Environment Lookups
**File:** src/eval/environment.rs  
**Complexity:** Medium  
**Improvement:** O(n) → O(1) lookup

```rust
use std::collections::HashMap;

pub struct StackFrame {
    vars: HashMap<&'static str, Value>,  // O(1) lookup
}

pub fn lookup(&self, name: &str) -> Option<Value> {
    for scope in self.scopes.iter().rev() {
        if let Some(value) = scope.vars.get(name) {
            return Some(value.clone());
        }
    }
    None
}
```

---

## IMPLEMENTATION ORDER

### Recommended Sequence:

1. **Day 1:** Fix 1-3 (Critical fixes) - 1 hour
   - Enable type checking
   - Fix warnings
   - Validate app entry point

2. **Day 1-2:** Fix 4-6 (High priority) - 2-2.5 hours
   - Use thiserror
   - Refactor main.rs
   - Add location tracking

3. **Day 2-3:** Fix 7-9 (Medium priority) - 40 minutes
   - Quick wins with big impact

4. **Later:** Fix 10-11 (Advanced) - Only if needed
   - Large refactoring
   - Performance optimization

---

## TESTING STRATEGY

After each fix:
```bash
# Run tests
cargo test

# Run hello_world
./target/release/rocflight hello_world/main.roc

# Check compiler output
cargo check
cargo clippy
```

---

## VERIFICATION

**Phase 1 Complete When:**
- ✅ Type checking enabled and catches errors
- ✅ No compiler warnings
- ✅ App entry point validation works
- ✅ All existing tests still pass

**Phase 2 Complete When:**
- ✅ Using thiserror crate
- ✅ main.rs refactored with Result
- ✅ Error messages show line/column
- ✅ All tests passing

**Phase 3 Complete When:**
- ✅ Parser encapsulation improved
- ✅ Code follows Rust idioms
- ✅ All tests passing

---

## Expected Outcomes

### Before:
```
warning: unused variable: `module`
warning: unused variable: `name`
Type: ($3 -> $5)
There are -3 birds.
App output: ""
Parse error: Parse error at position 21: Expected 'in' after value in let binding
```

### After:
```
There are -3 birds.
Error: Parse error at line 2, column 5: Expected 'in' after value in let binding
Error: App entry point 'main' not found
```

---

## Summary

| Fix | Time | Impact | Status |
|-----|------|--------|--------|
| Enable type checking | 30m | HIGH | Ready |
| Fix warnings | 15m | LOW | Ready |
| Validate app entry | 20m | HIGH | Ready |
| Use thiserror | 60m | MEDIUM | Ready |
| Refactor main.rs | 45m | MEDIUM | Ready |
| Add location tracking | 90m | MEDIUM | Ready |
| Minor improvements | 40m | LOW | Ready |
| Replace unsafe | (later) | MEDIUM | Design needed |
| Optimize lookups | (later) | LOW | Design needed |

**Total Phase 1-3: ~4 hours**  
**Achievable in one session**

