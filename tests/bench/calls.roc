# Function call overhead: naive fib is nothing but calls and arithmetic.
# expect: 17711
# The argument depends on `args.len()` so that roc cannot fold `fib(22)` at compile time.
app [main!] {}

fib : U64 -> U64
fib = |n| if n < 2 { n } else { fib(n - 1) + fib(n - 2) }

main! = |args| {
	echo!(U64.to_str(fib(22 + args.len())))
	Ok({})
}
