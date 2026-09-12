# Shorthand Syntax Desugaring Strategy

## Overview

Roc uses several shorthand syntaxes that represent functional operations. Instead of handling these specially in the parser, we will:

1. **Desugar all shorthand syntax** into explicit functional syntax
2. **Save the desugared code** to a temporary .roc file
3. **Parse the desugared file** using the standard nom parser
4. **Build AST** from clean, functional representation

This approach:
- ✅ Keeps parser simple (no special cases for `!`, `?`, `??`, `.?`)
- ✅ Maintains single source of truth (Roc compiler docs)
- ✅ Makes the AST cleaner and easier to evaluate
- ✅ Simplifies type checking (all syntax is functional)
- ✅ Easy to debug (can inspect desugared .roc files)

---

## Shorthand Syntax Reference

Source of truth: `/home/brian/Code/rocflight/roc-compiler/test/echo/all_syntax_test.roc`

### 1. Effectful Function Marker: `!`

**Shorthand:**
```roc
echo! = |msg| Stdout.write!(msg)
main! = |_args| echo!("Hello")
```

**Desugared (explicit Effect type):**
```roc
echo = |msg| Stdout.write!(msg)
main = |_args| echo("Hello")
```

**Conversion Rule:**
- Function name ending with `!` → remove `!` from name
- Type annotation with `=>` → convert to nested function type
- No change to semantics for v1.0 (all functions are "effectful")

**Roc Docs Reference:**
- Effect syntax: `Str => Result {} [IOErr]`
- Means: takes Str, produces effects, returns `Result {} [IOErr]`

---

### 2. Error Handling: `?` Operator

**Shorthand (early return on Err):**
```roc
first_str = strings.first()?
first_num = I64.from_str(first_str)?
```

**Desugared (explicit pattern matching):**
```roc
first_str = 
  match strings.first() {
    Ok(value) => value
    Err(err) => return Err(err)
  }
first_num = 
  match I64.from_str(first_str) {
    Ok(value) => value
    Err(err) => return Err(err)
  }
```

**With Error Mapping:**
```roc
# Shorthand: wraps error with tag
first_str = strings.first() ? NoFirstError

# Desugared:
first_str = 
  match strings.first() {
    Ok(value) => value
    Err(err) => return Err(NoFirstError(err))
  }
```

**With Error Lambda:**
```roc
# Shorthand: maps error with function
first_str = strings.first() ? |e| NoFirstError(e)

# Desugared:
first_str = 
  match strings.first() {
    Ok(value) => value
    Err(err) => return Err(|e| NoFirstError(e) err)
  }
```

**Conversion Rules:**
- `expr?` → `match expr { Ok(v) => v, Err(e) => return Err(e) }`
- `expr ? Tag` → `match expr { Ok(v) => v, Err(e) => return Err(Tag(e)) }`
- `expr ? |e| func(e)` → `match expr { Ok(v) => v, Err(e) => return Err(func(e)) }`

---

### 3. Default Value: `??` Operator

**Shorthand (provide default for Err):**
```roc
default: I64.from_str("not a number") ?? 0
```

**Desugared (explicit pattern matching):**
```roc
default: 
  match I64.from_str("not a number") {
    Ok(value) => value
    Err(_) => 0
  }
```

**Conversion Rule:**
- `expr ?? default_value` → `match expr { Ok(v) => v, Err(_) => default_value }`

---

### 4. Optional Field Access: `.?`

**Shorthand (access optional field):**
```roc
timeout_str = match config.?timeout_ms {
  Ok(ms) => "${ms.to_str()}ms"
  Err(MissingField) => "no timeout"
}
```

**Desugared (explicit field access with Try type):**
```roc
timeout_str = match 
  if timeout_ms_exists then 
    Ok(config.timeout_ms) 
  else 
    Err(MissingField)
{
  Ok(ms) => "${ms.to_str()}ms"
  Err(MissingField) => "no timeout"
}
```

**Conversion Rule:**
- `.?field` → Check field existence, return `Try` (Result with specific error)
- Requires tracking which fields are optional in record type

---

### 5. Optional Record Field Definition: `?:`

**Shorthand (optional field in record type):**
```roc
ServerConfig := { 
  host : Str, 
  port : U16 ?? 8080,      # default value
  timeout_ms ?: U64         # optional (no default)
}
```

**Desugared (explicit field metadata):**
```roc
ServerConfig := { 
  host : Str, 
  port : U16,           # has default: 8080
  timeout_ms : Try(U64) # optional, returns Try when accessed
}
```

**Conversion Rule:**
- `field : Type ??` default → field is optional with default
- `field ?: Type` → field is optional without default (Try return type)

---

## Implementation Strategy

### Step 1: Lexer-Level Desugaring (Before Parser)

Create a preprocessing step that:

```rust
pub struct Desugarer {
    input: String,
}

impl Desugarer {
    pub fn desugar(&self) -> Result<String, DesugarError> {
        // Pass 1: Remove all ! from function names and type annotations
        let step1 = self.desugar_effects();
        
        // Pass 2: Replace ? operators with match expressions
        let step2 = self.desugar_question_mark(&step1);
        
        // Pass 3: Replace ?? operators with match expressions
        let step3 = self.desugar_default(&step2);
        
        // Pass 4: Replace .? with Try-based access
        let step4 = self.desugar_optional_access(&step3);
        
        // Pass 5: Process optional fields (?:) in records
        let step5 = self.desugar_optional_fields(&step4);
        
        Ok(step5)
    }
}
```

### Step 2: Save Desugared Code

```rust
pub fn load_and_desugar(file_path: &str) -> Result<String, Error> {
    // Read original .roc file
    let source = std::fs::read_to_string(file_path)?;
    
    // Desugar all shorthand syntax
    let desugarer = Desugarer::new(source);
    let desugared = desugarer.desugar()?;
    
    // Save to temp file for inspection (debug builds)
    #[cfg(debug_assertions)]
    {
        let temp_path = format!("{}.desugared.roc", file_path);
        std::fs::write(&temp_path, &desugared)?;
        eprintln!("Desugared file saved to: {}", temp_path);
    }
    
    Ok(desugared)
}
```

### Step 3: Parse Desugared Code

```rust
pub fn parse_from_file(file_path: &str) -> Result<Expr, ParseError> {
    // Load and desugar
    let desugared_source = load_and_desugar(file_path)?;
    
    // Parse the clean, functional syntax
    let mut parser = Parser::new(&desugared_source);
    parser.parse_expr()
}
```

---

## Example: Full Desugaring Flow

### Input (hello_world/main.roc with shorthand)
```roc
app [main!] { pf: platform "https://...tar.br" }

import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${I64.to_str(birds)} birds.")
```

### After Desugaring Pass 1: Effects (remove `!`)
```roc
app [main] { pf: platform "https://...tar.br" }

import pf.Stdout

birds = -3

main = |_args|
  Stdout.line("There are ${I64.to_str(birds)} birds.")
```

### After Desugaring Pass 2-5: Other shorthand
(No `?`, `??`, `.?` in this example, so no further changes)

### Final Desugared Code
```roc
app [main] { pf: platform "https://...tar.br" }

import pf.Stdout

birds = -3

main = |_args|
  Stdout.line("There are ${I64.to_str(birds)} birds.")
```

### Parser Input
Clean, functional syntax ready for nom parser

### AST Output
Simple, no special cases for effects or error handling

---

## Source of Truth: Roc Compiler References

All desugaring rules verified against:

1. **all_syntax_test.roc**
   - `/home/brian/Code/rocflight/roc-compiler/test/echo/all_syntax_test.roc`
   - Shows all syntax examples
   - Verify types with: `roc check all_syntax_test.roc`

2. **Design Documentation**
   - `/home/brian/Code/rocflight/roc-compiler/design.md`
   - Architecture details
   - Type system documentation

3. **Compiler Source Code**
   - Reference implementation in Zig
   - Shows how shorthand is converted

---

## Benefits of This Approach

✅ **Simplicity:** Parser only handles functional syntax
✅ **Correctness:** Desugaring is separate from parsing logic
✅ **Debuggability:** Can inspect desugared .roc files
✅ **Maintainability:** Desugaring logic is isolated
✅ **Testability:** Test desugaring separately from parsing
✅ **Future-proof:** Easy to add more shorthand syntax

---

## Integration with Interpreter Phases

### Phase 1 Part A (Strings)
- Include: Basic desugaring infrastructure
- Not needed: All desugaring rules

### Phase 1 Part B (Platform Loading)
- Include: Effect desugaring (`!` removal)
- Enables: `main!` and `Stdout.line!()` to work

### Phase 2-3 (Numbers, Variables)
- Include: Full desugaring (all rules)
- Now supports: Complete Roc syntax

### Phase 7+ (Functions, Pattern Matching)
- Include: Error handling desugaring (`?`, `??`)
- Enables: Error handling patterns

---

## Testing Checklist

- [ ] Desugarer processes `!` correctly
- [ ] Desugarer processes `?` correctly
- [ ] Desugarer processes `??` correctly
- [ ] Desugarer processes `.?` correctly
- [ ] Desugarer processes `?:` correctly
- [ ] Desugared output parses without errors
- [ ] Original and desugared code produce same AST structure
- [ ] Temp file generation works (debug builds)
- [ ] Performance: desugaring < 1ms for typical files

---

## Roc Compiler Integration

If updates to Roc syntax occur in roc-compiler repo, refresh desugaring by:

1. Check updated `all_syntax_test.roc`
2. Run `roc check all_syntax_test.roc`
3. Update desugaring rules to match output
4. Add tests for new syntax
5. Update this document

---

## Example Desugaring Rules (Complete Reference)

| Shorthand | Desugared | Phase | Priority |
|-----------|-----------|-------|----------|
| `name!` | `name` | 1B | HIGH |
| `Type => Out` | `(Type) -> Out` | 1B | HIGH |
| `expr?` | `match expr { Ok(v)=>v, Err(e)=>return Err(e) }` | 7 | MEDIUM |
| `expr ?? def` | `match expr { Ok(v)=>v, Err(_)=>def }` | 7 | MEDIUM |
| `.?field` | `if field_exists then Ok(field) else Err(MissingField)` | 9 | MEDIUM |
| `field ?: Type` | Field type becomes Try-based | 9 | MEDIUM |
| `field : Type ?? def` | Field has default value | 9 | LOW |

