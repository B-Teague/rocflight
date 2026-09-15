# The same map and fold with TOP-LEVEL lambdas, which capture nothing. Read against
# expect: 16004000
# closure_capture.roc: the gap between them is environment-copying cost, not list cost.
app [main!] {}

double : I64 -> I64
double = |x| x * 2

add : I64, I64 -> I64
add = |a, x| a + x

main! = |_args| {
	echo!(I64.to_str((1..=4000).iter().map(double).fold(0, add)))
	Ok({})
}
