# Function call overhead: naive fib is nothing but calls and arithmetic.
# expect: 17711
app [main!] {}

fib : I64 -> I64
fib = |n| if n < 2 { n } else { fib(n - 1) + fib(n - 2) }

main! = |_args| {
	echo!(I64.to_str(fib(22)))
	Ok({})
}
