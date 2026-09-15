# Syntax: chained dispatch, and the receiver's position among the arguments.
#
# `xs.fold(seed, |a, x| a - x)` is `List.fold(xs, seed, f)` — subtraction is used here
# precisely because it would give a different answer if the arguments were swapped.
app [main!] {}

main! = |_args| {
    seed = I64.from_str("10") ?? 0
    xs = [1, 2]
    r = { s: "" }
    echo!("${xs.fold(seed, |a, x| a - x).to_str()},${Str.inspect(r.s.is_empty())},${xs.map(|x| x).len().to_str()}")
    Ok({})
}
