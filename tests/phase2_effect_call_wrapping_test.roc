# Test Phase 2: Effect Call Wrapping
# Demonstrates how ! operator is desugared to match expressions

# Simple effectful call
test_simple = Stdout.line!("hello")

# Nested effectful calls  
test_nested = Result.map!(
  Stdout.line!("message"),
  |msg| Num.to_str(msg)
)

# Module-qualified effectful call
test_module = Http.get!("https://example.com")

# Multiple effectful calls in sequence
test_sequence = |_|
  Stdout.line!("First")
  Stdout.line!("Second")
  Stdout.line!("Third")

# Effectful function definition (removes !)
echo! = |msg| Stdout.line!(msg)
