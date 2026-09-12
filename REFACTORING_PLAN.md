# Refactoring Plan - Applying Golden Rules

**Date:** 2026-09-11  
**Objective:** Improve code quality against the 10 golden rules  
**Status:** In Progress

---

## Issues Identified

### Rule 1: Handle Every Error Intentionally
**Violations Found:** 11 instances of `.unwrap()` on mutexes and tests

**Locations:**
- `src/memory/string_pool.rs:55` - `GLOBAL_POOL.lock().unwrap()`
- `src/platform/cache.rs:73,79,85` - Multiple mutex unwraps
- `src/parser/mod.rs:485,493` - `.chars().next().unwrap()`
- `src/desugaring/mod.rs:166,176,186,197` - Test unwraps

**Solution:** 
- Replace `.lock().unwrap()` with proper error handling or use `expect()` with message
- Replace `.chars().next().unwrap()` with safe alternatives

**Priority:** HIGH - Prevents panics in production

---

### Rule 2: Clone Only When You Have a Reason
**Violations Found:** 17 instances of `.clone()`

**Locations:**
- `src/platform/cache.rs:33,43` - Cloning platform (expensive)
- `src/eval/mod.rs:108,111,113,146,199` - Cloning in lambda handling
- `src/parser/mod.rs:42,56,232` - Cloning strings and options
- Others in type checking and platform modules

**Solution:**
- Use references where ownership not needed
- For cache.get(), return &Platform instead of cloned Platform
- For params/env in lambdas, evaluate necessity

**Priority:** MEDIUM - Performance improvement

---

### Rule 3: Don't Fight Ownership; Simplify Design
**Violations Found:** 2 instances of complex ownership patterns

**Locations:**
- `src/eval/mod.rs:108` - Transmute due to lifetime constraints
- `src/parser/mod.rs:42` - Deep cloning desugarer input

**Solution:**
- Refactor lambda closure handling to use Rc<Expr> or better lifetime model
- Streamline parser ownership

**Priority:** MEDIUM - Design improvement

---

### Rule 6: Borrow When You Don't Need Ownership
**Violations Found:** Multiple functions taking owned values unnecessarily

**Locations:**
- `src/platform/cache.rs:37` - Takes owned String for URL
- `src/platform/cache.rs:78` - Takes owned String, platform
- Parser methods passing owned Expr unnecessarily

**Solution:**
- Use `&str` for URL parameters
- Use `&T` for non-mutating references
- Only take ownership when needed (move semantics)

**Priority:** HIGH - API clarity

---

### Rule 9: Keep Unsafe Code Tiny and Justified
**Status:** Currently acceptable (1 transmute with good comment)

**Location:** `src/eval/mod.rs:107-108`

**Current State:**
- ✅ Justified with SAFETY comment
- ✅ Limited to one place
- ⚠️ Could be improved with Rc-based design

**Solution:** Evaluate if Rc<Expr> pattern would eliminate unsafe code

**Priority:** LOW - Currently acceptable but improvable

---

## Refactoring Order

### Phase 1 (Critical - Do First)
1. ✅ Replace `.lock().unwrap()` with proper error handling
2. ✅ Fix parser unsafe `.chars().next().unwrap()` calls
3. ✅ Change function signatures to accept `&str` instead of owned String

### Phase 2 (Important - Do Second)  
1. Remove unnecessary `.clone()` calls in platform cache
2. Simplify parser string handling
3. Fix test unwraps

### Phase 3 (Nice to Have)
1. Evaluate Rc-based lambda closure handling
2. Profile clone impact and optimize hotspots

---

## Files to Refactor (in priority order)

1. **src/platform/cache.rs** - Many clones, unwraps
2. **src/parser/mod.rs** - String ownership, unsafe chars()
3. **src/eval/mod.rs** - Lambda clone, transmute
4. **src/memory/string_pool.rs** - Mutex unwrap
5. **src/desugaring/mod.rs** - Test unwraps

---

## Success Criteria

- [ ] Zero `.lock().unwrap()` on non-test code
- [ ] Reduce `.clone()` calls by 50%
- [ ] All functions use appropriate borrowing (`&T` vs owned `T`)
- [ ] All tests pass
- [ ] Zero new compiler warnings

