# Syntax: `|>` binds TIGHTER than every binary operator.
#
# That is the opposite of most languages, where a pipe is the loosest thing in an
# expression. Here `1 + 2 |> inc` is `1 + inc(2)`, giving 4 — not `inc(1 + 2)`.
app [main!] {}

inc = |n| n + 1

main! = |_args| {
    a = 1 + 2 |> inc
    b = 2 * 3 |> inc
    echo!("${I64.to_str(a)},${I64.to_str(b)}")
    Ok({})
}
