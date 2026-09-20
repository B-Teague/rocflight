# The environment a closure captures. Identical work to list_ops.roc, except the
# expect: 16004000
# list is bound in the SAME block as the lambda, so it is part of what the closure
# captures. Any per-call copy of the environment shows up here and nowhere else.
app [main!] {}

main! = |_args| {
	xs = (1..=4000).iter()
	doubled = xs.map(|x| x * 2)
	echo!(I64.to_str(doubled.fold(0, |a, x| a + x)))
	Ok({})
}
