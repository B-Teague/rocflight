main = |_args|
match Stdout.line("Hello, World!") { Ok(v) => v, Err(e) => return Err(e) }