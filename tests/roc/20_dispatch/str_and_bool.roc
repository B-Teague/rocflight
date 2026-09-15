# Syntax: dispatch on Str and on a zero-argument method.
app [main!] {}

main! = |_args| {
    empty = ""
    filled = "x"
    echo!("${Str.inspect(empty.is_empty())},${Str.inspect(filled.is_empty())}")
    Ok({})
}
