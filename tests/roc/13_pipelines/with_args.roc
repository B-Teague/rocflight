# Syntax: piping into a call that already has arguments.
#
# `x |> f(a)` is `f(x, a)` — the piped value is PREPENDED, the same convention static
# dispatch uses. Subtraction is used below because it would give a different answer if
# the order were reversed.
app [main!] {}

subtract = |a, b| a - b

main! = |_args| {
    n = 10
    echo!(I64.to_str(n |> subtract(4)))
    Ok({})
}
