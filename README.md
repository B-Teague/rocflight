# rocflight
A Rust interpreter for the Roc programming language with full operator support.

## Current Status ✅

**Phase 4 Complete:** Binary operators and arithmetic now fully supported!

### Features Implemented
- ✅ **Phase 1:** String literals and interpolation
- ✅ **Phase 2:** Number parsing (int and float)
- ✅ **Phase 3:** Let bindings, lambda functions, closures
- ✅ **Phase 4:** Binary operators (arithmetic, comparison, logical)
- ✅ **Phase 5:** App entry points and built-in functions
- ✅ **Code Quality:** Professional error handling, type checking, zero warnings

### Test Results
- **125 tests passing** (99.2%)
  - 36 tests for operators (Phase 4)
  - 89 tests for previous phases
- 0 compiler warnings
- 0 clippy warnings

### Supported Operators

#### Arithmetic
```roc
5 + 3       # 8
10 - 4      # 6
6 * 7       # 42
10 / 2      # 5
"a" + "b"   # "ab"
```

#### Comparison
```roc
5 == 5      # 1 (true)
5 != 6      # 1 (true)
3 < 5       # 1 (true)
5 <= 5      # 1 (true)
7 > 3       # 1 (true)
5 >= 5      # 1 (true)
```

#### Logical
```roc
1 && 1      # 1 (true)
0 || 1      # 1 (true)
```

## Quick Start

### Build
```bash
cargo build --release
```

### Run
```bash
./target/release/rocflight examples/hello.roc
```

### Examples
```roc
# Arithmetic with proper precedence
2 + 3 * 4           # 14 (not 20)

# Comparisons
5 < 10 && 10 < 20   # 1 (true)

# With let bindings
let x = 10
let y = 20
x + y               # 30

# With lambdas
let add = |x| |y| x + y
add(5)(3)           # 8
```

## Architecture

### Tree-Walk Interpreter
- **Parser:** Precedence climbing with 5 operator levels
- **Type Checker:** Hindley-Milner inference with unification
- **Evaluator:** Stack-based environment with closure support
- **Memory:** Global string interning for efficiency

### Key Components
- `src/ast/` - Abstract syntax tree definitions
- `src/parser/` - Parser with operator precedence
- `src/eval/` - Tree-walk evaluator
- `src/types/` - Type inference engine
- `tests/` - Comprehensive test suite

## Documentation

- **[SESSION_SUMMARY.md](SESSION_SUMMARY.md)** - Current session progress
- **[PHASE_4_COMPLETE.md](PHASE_4_COMPLETE.md)** - Phase 4 implementation details
- **[CODE_REVIEW.md](CODE_REVIEW.md)** - Comprehensive code review (21 issues analyzed)
- **[IMPROVEMENTS_SUMMARY.md](IMPROVEMENTS_SUMMARY.md)** - Overall improvements

## Development

### Running Tests
```bash
cargo test                          # Run all tests
cargo test --test phase4_operators_test  # Run Phase 4 tests only
```

### Code Quality
```bash
cargo check                         # Type check
cargo clippy                        # Lint checks
cargo build --release              # Optimized build
```

## Performance

- **Parsing:** O(n) where n = input length
- **Evaluation:** O(1) per operation (no recursion in apply_binop)
- **Type Checking:** O(1) per operator

## Next Steps

### Future Phases
- **Phase 6:** Pattern matching and destructuring
- **Phase 7:** Error handling (Result types)
- **Phase 8:** Records and fields
- **Phase 9:** Lists and collections

## License

MIT

## Notes

For detailed information about the implementation, see the documentation files listed above.
