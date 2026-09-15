# Syntax: dispatch on a List receiver, including extra arguments and chaining.
app [main!] {}

main! = |_args| {
    seed = I64.from_str("0") ?? 0
    xs = [1, 2, 3]
    echo!("${xs.len().to_str()},${Str.inspect(xs.map(|x| x * 2))},${xs.fold(seed, |a, x| a + x).to_str()}")
    Ok({})
}
