# A `dbg` is printed by the host's own `roc_dbg`, with the real argument count.
# basic-cli's host then exits 1 for a program that used `dbg`, and so does this.
app [main!] {
	cli: platform "https://github.com/roc-lang/basic-cli/releases/download/0.22.0/F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL.tar.zst",
	roc: "nightly-2026-09-03-62fcb65",
}

main! = |args| {
	count = List.len(args)
	dbg count
	Ok({})
}
