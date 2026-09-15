# Syntax: the pipeline operator — `x |> f` is `f(x)`.
app [main!] {}

double = |n| n * 2

main! = |_args| {
    n = 21
    echo!(I64.to_str(n |> double))
    Ok({})
}
