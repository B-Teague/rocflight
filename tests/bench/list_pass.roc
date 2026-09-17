# Passing a list to a function. Every argument is a register move, and a move used to
# expect: 8002000
# deep-copy a list — so a loop that handed the same 4,000 elements to each iteration
# copied them 4,000 times. A list is refcounted now, and this stays linear.
app [main!] {}

sum_at : List(I64), I64, I64 -> I64
sum_at = |xs, i, acc| if i == xs.len() { acc } else { sum_at(xs, i + 1, acc + xs.get(i).with_default(0)) }

main! = |_args| {
	xs = (1..=4000).iter().map(|x| x)
	echo!(I64.to_str(sum_at(xs, 0, 0)))
	Ok({})
}
