# Pattern matching and tag construction.
# expect: 215998199970000
app [main!] {}

Shape : [Circle(I64), Rect(I64, I64), Dot]

area : Shape -> I64
area = |s| match s {
	Circle(r) => 3 * r * r
	Rect(w, h) => w * h
	Dot => 0
}

main! = |_args| {
	var total = 0
	for i in 0..<60000 {
		total = total + area(Rect(i, 2)) + area(Circle(i)) + area(Dot)
	}
	echo!(I64.to_str(total))
	Ok({})
}
