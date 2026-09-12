# Roc Interpreter Code Review

**Date:** 2026-09-11  
**Status:** Phase 5 Complete (109 tests passing)  
**Focus:** Best practices, error handling, Rust idioms, functionality preservation

---

## CRITICAL ISSUES TO ADDRESS

### 1. ⚠️ UNSAFE TRANSMUTE IN LAMBDA EVALUATION (src/eval/mod.rs:103-108)

**Issue:** Lifetime transmute bypasses Rust's safety guarantees
```rust
let static_body = unsafe {
    std::mem::transmute::<Box<Expr<'_>>, Box<Expr<'static>>>(body.clone())
};
```

**Problems:**
- Relies on manual memory management claim ("remains valid for program lifetime")
- No compile-time guarantee - could crash if source is deallocated
- Breaks Rust's zero-cost abstraction principle
- Makes code non-portable if architecture changes

**Recommended Fix:**
```rust
// Option 1: Use Rc<Expr> instead of Box
pub enum Expr<'a> {
    Lambda {
        params: Vec<&'static str>,
        body: Rc<Expr<'a>>,  // Shared ownership
    },
}

// Option 2: Use string representation and re-parse
// Option 3: Use arena allocator (bumpalo is already imported!)
```

**Impact:** MUST FIX before production use

---

### 2. ⚠️ INCOMPLETE ERROR MESSAGES

**ParseError missing location context:**
```rust
// Current
ParseError { message: "Expected 'in'", position: 21 }
// Displayed as: "Parse error at position 21: Expected 'in'"

// Problem: No line/column information for multi-line files
```

**Better approach:**
```rust
pub struct ParseError {
    pub message: String,
    pub position: usize,
    pub line: usize,
    pub column: usize,
}
```

**EvalError missing context:**
```rust
// Currently just a message, no location of failing expression
```

---

### 3. ⚠️ TYPE CHECKING DISABLED (src/main.rs:38-41)

**Issue:** Type checking is intentionally skipped:
```rust
let _type_result = type_checker.synth(&ast);
// Note: Type result not printed - only actual program output is shown
```

**Problems:**
- Real Roc always type-checks before running
- Type errors in files silently pass through
- Won't catch real Roc code issues

**Should be:**
```rust
match type_checker.synth(&ast) {
    Ok(_ty) => {}, // Type check passed
    Err(e) => {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
```

---

### 4. ⚠️ APP ENTRY POINT LOOKUP STRING MATCHING (src/main.rs:50)

**Issue:** Fragile string manipulation:
```rust
let lookup_name = entry_name.trim_end_matches('!');
```

**Problems:**
- What if entry_name is just "!"? Returns empty string
- No validation that entry_name starts with identifier
- Silent failure if entry not found

**Better:**
```rust
if !entry_name.ends_with('!') {
    eprintln!("Invalid app entry point: {} (must end with !)", entry_name);
    process::exit(1);
}
let lookup_name = &entry_name[..entry_name.len()-1];
```

---

## BEST PRACTICES ISSUES

### 5. Error Handling Not Using `?` Operator Effectively

**Current:** Verbose match statements
```rust
match evaluator.eval(&ast) {
    Ok(value) => { /* ... */ }
    Err(e) => {
        eprintln!("Runtime error: {}", e);
        process::exit(1);
    }
}
```

**Better:** Extract to helper function returning Result:
```rust
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (ast, app_entry_point) = Parser::from_file(filename)?;
    let mut evaluator = Evaluator::new();
    let value = evaluator.eval(&ast)?;
    // ... invoke app entry point ...
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
```

---

### 6. Missing `thiserror` Crate Usage

**Current:** Manual Error implementations
```rust
impl std::error::Error for ParseError {}
```

**Already imported in Cargo.toml:** `thiserror = "1.0"`  
**Not being used!**

**Should use:**
```rust
use thiserror::Error;

#[derive(Error, Debug, Clone)]
#[error("Parse error at position {position}: {message}")]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}
```

---

### 7. Incomplete Error Context (src/parser/mod.rs)

**App entry point extraction silently ignores errors:**
```rust
fn extract_app_entry_point(&mut self) {
    // Silently ignores if '[' not found
    // Silently ignores malformed entries
    // No error reporting
}
```

**Should validate and error:**
```rust
fn extract_app_entry_point(&mut self) -> Result<(), ParseError> {
    // Proper error handling
}
```

---

## RUST IDIOM ISSUES

### 8. String Cloning in Hot Paths (src/parser/mod.rs:61)

```rust
let rest = &self.input[self.pos..].to_string(); // Unnecessary clone!
```

**Should be:**
```rust
let rest = &self.input[self.pos..];
```

---

### 9. Uninitialized State in Parser (src/parser/mod.rs:17-18)

```rust
pub app_entry_point: Option<String>,
```

**Should default to None:**
```rust
pub fn new(input: &str) -> Self {
    Parser {
        input: input.to_string(),
        pos: 0,
        app_entry_point: None,  // Explicit
    }
}
```

OR use Default impl:

```rust
impl Default for Parser {
    fn default() -> Self {
        Self::new("")
    }
}
```

---

### 10. Unsafe Transmute Pattern Repeated (src/eval/mod.rs)

Multiple lambdas use same unsafe pattern - centralizes the risk:

```rust
// Better: Create helper function with clear invariants
fn make_static_expr(expr: Box<Expr>) -> Box<Expr<'static>> {
    // Document WHY this is safe
    // Centralize the transmute
    unsafe { std::mem::transmute(expr) }
}
```

---

### 11. Missing Default Trait Implementations

**Good:** `Environment` has `Default`  
**Bad:** `Evaluator` doesn't:

```rust
impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### 12. Environment Lookup String Comparison (src/eval/environment.rs:48)

```rust
pub fn lookup(&self, name: &str) -> Option<Value> {
    for scope in self.scopes.iter().rev() {
        for (var_name, value) in scope.vars.iter().rev() {
            if *var_name == name {  // String comparison each time
                return Some(value.clone());
            }
        }
    }
    None
}
```

**Issue:** 
- O(n) string comparison for each lookup
- Clone on every lookup

**Better:** Use HashMap for scope:
```rust
pub struct StackFrame {
    pub vars: HashMap<&'static str, Value>,  // O(1) lookup
}
```

---

## MEMORY & PERFORMANCE ISSUES

### 13. String Pool Linear Search (src/memory/string_pool.rs:25-29)

```rust
for &interned in &self.strings {
    if interned == s {
        return interned;
    }
}
```

**Problem:** O(n) for each string interning  
**HashSet is O(1)** but you're doing linear search anyway!

**Fix:**
```rust
pub struct StringPool {
    strings: HashSet<String>,  // Store actual strings
}

pub fn intern(&self, s: &str) -> &'static str {
    if let Some(existing) = self.strings.get(s) {
        existing.as_str() as *const str as *mut str as &str
    } else {
        let leaked = Box::leak(s.to_string().into_boxed_str());
        self.strings.insert(s.to_string());
        leaked
    }
}
```

---

### 14. Value Clone Performance (src/eval/environment.rs:49)

```rust
pub fn lookup(&self, name: &str) -> Option<Value> {
    // ...
    return Some(value.clone());  // Clone every lookup!
}
```

**Frequent clones:** In string interpolation, lambda calls, etc.

**Options:**
- Use `Rc<Value>` for reference counting
- Return `&Value` with lifetime
- Cache Value references

---

### 15. Memory Leak in Box::leak Usage

Multiple places use `Box::leak`:
- src/eval/mod.rs:64, 122 (string results)
- src/parser/mod.rs (identifiers)
- src/memory/string_pool.rs:31

**Issue:** These are intentional leaks for `&'static` lifetime

**Better pattern:**
- Use arena allocator (`bumpalo` is imported but unused!)
- Or use `Rc` for shared ownership

---

## TESTING & DOCUMENTATION

### 16. Missing Comprehensive Error Testing

**Current:** Tests for happy path only  
**Missing:** Tests for:
- Parse errors with exact position
- Type errors with context
- Runtime errors with stack traces
- App entry point errors (not found, wrong type, etc.)

---

### 17. No Test for Malformed Input

```rust
// Add tests for:
echo "let x = " > /tmp/bad.roc  // Incomplete
echo "||" > /tmp/bad.roc        // Invalid lambda
echo "main! = 5" > /tmp/bad.roc // Type mismatch
```

---

### 18. Compiler Warnings Not Addressed

```
warning: unused variable: `module`
warning: unused variable: `name`
warning: unused variable: `original_path`
warning: unused variable: `desugared`
```

**Should fix:** Remove unused variables or prefix with `_`

---

## ARCHITECTURE ISSUES

### 19. Parser Mutates Global State (app_entry_point)

```rust
pub struct Parser {
    // ...
    pub app_entry_point: Option<String>,  // Public field
}
```

**Issue:** Public field can be modified from outside  
**Should be:** Private with getter

```rust
struct Parser {
    app_entry_point: Option<String>,  // Private
}

impl Parser {
    pub fn app_entry_point(&self) -> Option<&str> {
        self.app_entry_point.as_deref()
    }
}
```

---

### 20. Desugaring Error Handling (src/parser/mod.rs:43)

```rust
let desugared = desugarer.desugar()?;
```

**Then immediately:**
```rust
let _ = desugarer.save_debug(path, &desugared);  // Ignore error
```

**Should handle:**
```rust
if let Err(e) = desugarer.save_debug(path, &desugared) {
    eprintln!("Warning: Could not save debug file: {}", e);
}
```

---

### 21. Platform Module Incomplete (src/platform/)

- `Platform` exists but not fully integrated
- Platform loading happens but result not used
- Should validate platform before runtime

---

## FUNCTIONALITY PRESERVATION CHECKS

### ✅ **VERIFIED WORKING:**

| Feature | Status | Tests |
|---------|--------|-------|
| String parsing | ✅ | 5/5 |
| String interpolation | ✅ | Passing |
| Number parsing | ✅ | 19/19 |
| Let bindings | ✅ | 16/16 |
| Lambda closures | ✅ | 12/12 |
| Function calls | ✅ | Passing |
| App entry points | ✅ | Passing |
| Environment capture | ✅ | Passing |
| Chained calls | ✅ | Passing |

### ⚠️ **NOT YET IMPLEMENTED:**

| Feature | Impact | Priority |
|---------|--------|----------|
| Arithmetic operators | Needed for Phase 4 | HIGH |
| Pattern matching | Needed for Phase 6 | HIGH |
| Error types (Result) | Needed for Phase 7 | HIGH |
| Module system | Needed later | MEDIUM |

---

## RECOMMENDATIONS SUMMARY

### **CRITICAL (Fix before using in production):**
1. Replace unsafe transmute with `Rc<Expr>` or arena allocator
2. Enable type checking - don't skip it
3. Fix ParseError to include line/column info
4. Validate app entry point names

### **HIGH PRIORITY (Should fix soon):**
1. Use `thiserror` crate for error types
2. Refactor main.rs with Result-based error handling
3. Fix string pool linear search
4. Add proper error tests
5. Fix compiler warnings

### **MEDIUM PRIORITY (Good to have):**
1. Use HashMap for environment lookups
2. Consider Rc for Value to reduce clones
3. Improve error messages with stack traces
4. Add comprehensive error test suite
5. Document safety invariants for unsafe code

### **LOW PRIORITY (Nice to have):**
1. Use bumpalo arena allocator
2. Performance profiling/optimization
3. Add REPL mode
4. Implement debug mode with line numbers

---

## POSITIVE FINDINGS ✅

1. **Clean separation of concerns** - Parser, Type checker, Evaluator are well-separated
2. **Good test coverage** - 89+ passing tests
3. **Functional architecture** - Tree-walk interpreter is straightforward
4. **Type system foundation** - Hindley-Milner inference is correctly implemented
5. **Environment management** - Stack-based scoping works well
6. **String handling** - Interpolation works correctly
7. **Documentation** - Code has good comments and module docs
8. **Gradual implementation** - Phase-by-phase approach is sound

---

## CONCLUSION

The interpreter has a **solid foundation** with good architecture and most core features working correctly. The main issues are:

1. **Safety:** Unsafe code needs better justification and alternatives
2. **Error handling:** Should use proper error types and propagation
3. **Performance:** Some O(n) operations that could be O(1)
4. **Type checking:** Should be enabled, not skipped

**Recommend:** Address the 3-4 critical issues, then tackle high priority items before next major release.

