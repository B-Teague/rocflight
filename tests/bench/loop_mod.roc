# loop.roc's work as a MODULE, so both engines can run it: the same `var` and `for`,
# expect: 19999900000
# with the module's own value in place of `echo!`. No allocation, no calls.
run = || {
	var total = 0
	for i in 0..<200000 {
		total = total + i
	}
	total
}

run()
