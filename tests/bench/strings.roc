# String building. Every intermediate is a fresh allocation; this is where a leaked
# expect: 16000
# or over-copied string representation shows up.
app [main!] {}

main! = |_args| {
	var acc = ""
	for _i in 0..<8000 {
		acc = acc.concat("xy")
	}
	echo!(U64.to_str(acc.count_utf8_bytes()))
	Ok({})
}
