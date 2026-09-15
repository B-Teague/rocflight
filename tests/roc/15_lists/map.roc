# Syntax: List.map(list, fn) — the list comes first.
app [main!] {}

doubled = |xs| List.map(xs, |x| x * 2)

main! = |_args| {
    xs = [1, 2, 3]
    echo!("${Str.inspect(doubled(xs))},${Str.inspect(doubled([]))},${I64.to_str(xs.fold(0, |a, x| a + x))}")
    Ok({})
}
