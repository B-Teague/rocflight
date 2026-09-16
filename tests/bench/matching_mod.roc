# matching.roc's work as a MODULE — the tree-walker's own formulation, `var` plus
# expect: 215998199970000
# `for`, so the two engines run the SAME program. 180,000 tag constructions and matches.
Shape : [Circle(I64), Rect(I64, I64), Dot]

area : Shape -> I64
area = |s| match s {
	Circle(r) => 3 * r * r
	Rect(w, h) => w * h
	Dot => 0
}

run = || {
	var total = 0
	for i in 0..<60000 {
		total = total + area(Rect(i, 2)) + area(Circle(i)) + area(Dot)
	}
	total
}

run()
