# matching.roc's work, written so BOTH engines can run it: a tail-recursive loop
# expect: 215998199970000
# instead of `var` + `for`, and the module's own value instead of `echo!`. Tag
# construction and matching, 180,000 of each.
Shape : [Circle(I64), Rect(I64, I64), Dot]

area : Shape -> I64
area = |s| match s {
	Circle(r) => 3 * r * r
	Rect(w, h) => w * h
	Dot => 0
}

go = |i, total| if i == 60000 { total } else { go(i + 1, total + area(Rect(i, 2)) + area(Circle(i)) + area(Dot)) }

go(0, 0)
