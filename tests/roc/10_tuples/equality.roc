# Syntax: tuple equality is element-wise.
app [main!] {}

same = |a, b| a == b

main! = |_args| {
    echo!("${Str.inspect(same((1, "x"), (1, "x")))},${Str.inspect(same((1, "x"), (2, "x")))}")
    Ok({})
}
