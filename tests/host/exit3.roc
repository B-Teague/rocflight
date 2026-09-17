# `Err(Exit(3))` from `main!` is the process exit code, as under `roc`.
app [main!] {
	cli: platform "https://github.com/roc-lang/basic-cli/releases/download/0.22.0/F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL.tar.zst",
	roc: "nightly-2026-09-03-62fcb65",
}

main! = |_args|
	Err(Exit(3))
