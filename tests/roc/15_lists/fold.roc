# Syntax: List.fold(list, initial, fn) — the accumulator is the fn's first argument.
app [main!] {}

total = |xs| List.fold(xs, 0, |acc, x| acc + x)

main! = |_args| {
    echo!("${I64.to_str(total([1, 2, 3, 4]))},${I64.to_str(total([]))}")
    Ok({})
}
