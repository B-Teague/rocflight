# Syntax: an app backed by a REAL platform, not the default host.
#
# The header names a platform by URL and pins the compiler. `roc` downloads, verifies
# and extracts it; the interpreter reads what roc left in ~/.cache/roc/packages.
#
# Requires the platform to be cached. If it is not, run `roc check` on this file once.
app [main!] {
	cli: platform "https://github.com/roc-lang/basic-cli/releases/download/0.22.0/F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL.tar.zst",
	roc: "nightly-2026-09-03-62fcb65",
}

import cli.Stdout

main! = |_args| {
	Stdout.line!("hello from a real platform")
}
