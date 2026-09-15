# Syntax: chained pipelines, left to right.
app [main!] {}

double = |n| n * 2

inc = |n| n + 1

main! = |_args| {
    n = 5
    echo!(I64.to_str(n |> double |> inc))
    Ok({})
}
