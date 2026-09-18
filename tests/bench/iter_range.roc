# `.iter()` over a large range, folded. Building the range as a list cost 190 MB and
# expect: 2000001000000.0
# 208ms for two million elements; a range that stays a range costs neither.
app [main!] {}

main! = |_args| {
	xs = (1..=2000000).iter()
	echo!(Dec.to_str(xs.fold(0, |a, x| a + x)))
	Ok({})
}
