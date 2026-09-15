# Syntax: lambda |x| body, and multi-arg |x, y| body
app [main!] {}

inc = |x| x + 1
add = |x, y| x + y

main! = |_args| {
    echo!("${I64.to_str(inc(41))} ${I64.to_str(add(20, 22))}")
    Ok({})
}
