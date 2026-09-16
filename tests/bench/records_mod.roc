# records.roc's work as a MODULE, `var` plus `for` as the tree-walker prefers it.
# expect: 40000
# 40,000 record updates and field reads.
Point : { x: I64, y: I64 }

step : Point -> Point
step = |p| { ..p, x: p.x + p.y }

run = || {
	var p = { x: 0, y: 1 }
	for _i in 0..<40000 {
		p = step(p)
	}
	p.x
}

run()
