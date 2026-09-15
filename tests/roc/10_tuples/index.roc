# Syntax: positional access with .0 / .1 — zero-based.
app [main!] {}

pair = ("Roc", 1)

mk = |n| (n, n + 1)

main! = |_args| {
    echo!("${pair.0},${I64.to_str(pair.1)},${I64.to_str(mk(5).1)}")
    Ok({})
}
