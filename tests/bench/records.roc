# Record construction, field access and update — the shapes most roc code is made of.
# expect: 40000
app [main!] {}

Point : { x: I64, y: I64 }

step : Point -> Point
step = |p| { ..p, x: p.x + p.y }

main! = |_args| {
	var p = { x: 0, y: 1 }
	for _i in 0..<40000 {
		p = step(p)
	}
	echo!(I64.to_str(p.x))
	Ok({})
}
