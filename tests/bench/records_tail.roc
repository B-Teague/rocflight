# records.roc's work as a tail-recursive loop instead of `var` + `for`: 40,000 record
# expect: 40000
# updates and field reads, with a frame-reusing call per step.
# The loop starts from `args.len()` so that roc cannot fold `go(0, ..)` at compile time.
app [main!] {}

go : U64, { x : I64, y : I64 } -> I64
go = |i, point| if i == 40000 { point.x } else { go(i + 1, { ..point, x: point.x + 1, y: point.y - 1 }) }

main! = |args| {
	echo!(I64.to_str(go(args.len(), { x: 0, y: 0 })))
	Ok({})
}
