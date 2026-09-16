# Creating a closure, rather than calling one. The lambda is built inside the loop, so
# expect: 9001125019995
# its body is turned into a runtime value 30,000 times. A body of any size shows up
# here: evaluating `Expr::Lambda` used to DEEP CLONE every AST node in it, per closure.
app [main!] {}

main! = |_args| {
	var total = 0
	for i in 0..<30000 {
		f = |x| (x * 2) + (x * 3) - (x // 2) + (x % 7) + (x * x) - (x + 1)
		total = total + f(i)
	}
	echo!(I64.to_str(total))
	Ok({})
}
