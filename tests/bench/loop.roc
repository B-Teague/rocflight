# Loop and mutable-variable overhead: no allocation, no calls.
# expect: 19999900000
app [main!] {}

main! = |_args| {
	var total = 0
	for i in 0..<200000 {
		total = total + i
	}
	echo!(I64.to_str(total))
	Ok({})
}
