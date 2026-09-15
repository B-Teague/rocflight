# Syntax: operator precedence (* before +, parens override)
app [main!] {}

main! = |_args| {
    a = 2 + 3 * 4
    b = (2 + 3) * 4
    echo!("${I64.to_str(a)} ${I64.to_str(b)}")
    Ok({})
}
