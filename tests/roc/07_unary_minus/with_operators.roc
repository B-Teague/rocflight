# Syntax: unary minus alongside the binary operators.
#
# `|>` binds tighter, so `-n |> inc` is `-(inc(n))`.
app [main!] {}

inc = |x| x + 1

main! = |_args| {
    n = 5
    a = 2 * -n
    b = 10 - -n
    c = -n |> inc
    echo!("${I64.to_str(a)},${I64.to_str(b)},${I64.to_str(c)}")
    Ok({})
}
