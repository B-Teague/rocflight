# matching.roc's work as a tail-recursive loop instead of `var` + `for`: the same tag
# expect: 215998199970000
# construction and matching, 180,000 of each, with a frame-reusing call per step.
# The loop starts from `args.len()` so that roc cannot fold `go(0, 0)` at compile time.
app [main!] {}

Shape : [Circle(U64), Rect(U64, U64), Dot]

area : Shape -> U64
area = |s| match s {
	Circle(r) => 3 * r * r
	Rect(w, h) => w * h
	Dot => 0
}

go : U64, U64 -> U64
go = |i, total| if i == 60000 { total } else { go(i + 1, total + area(Rect(i, 2)) + area(Circle(i)) + area(Dot)) }

main! = |args| {
	echo!(U64.to_str(go(args.len(), 0)))
	Ok({})
}
