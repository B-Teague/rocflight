# Syntax: list equality (element-wise) and lists of lists.
app [main!] {}

same = |a, b| a == b

nested = [[1], [2, 3]]

main! = |_args| {
    echo!("${Str.inspect(same([1, 2], [1, 2]))},${Str.inspect(same([1], [2]))},${Str.inspect(nested)},${I64.to_str(nested.fold(0, |a, xs| a + xs.fold(0, |b, x| b + x)))}")
    Ok({})
}
