# Test Phase 2: Effect Call Wrapping
# Demonstrates how  operator is desugared to match expressions

# Simple effectful call
test_simple = match Stdout.line("hello") { Ok(v) => v, Err(e) => return Err(e) }

# Nested effectful calls
test_nested = match Result.map(
Stdout.line!("message"),
|msg| Num.to_str(msg)
) { Ok(v) => v, Err(e) => return Err(e) }

# Module-qualified effectful call
test_module = match Http.get("https://example.com") { Ok(v) => v, Err(e) => return Err(e) }

# Multiple effectful calls in sequence
test_sequence = |_|
match Stdout.line("First") { Ok(v) => v, Err(e) => return Err(e) }
match Stdout.line("Second") { Ok(v) => v, Err(e) => return Err(e) }
match Stdout.line("Third") { Ok(v) => v, Err(e) => return Err(e) }

# Effectful function definition (removes )
echo = |msg| match Stdout.line(msg) { Ok(v) => v, Err(e) => return Err(e) }