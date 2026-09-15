# Syntax: + - * / on numbers
app [main!] {}

main! = |_args| {
    sum = 5 + 3
    diff = 10 - 4
    prod = 6 * 7
    quot = 7.0 / 2.0
    echo!("${I64.to_str(sum)} ${I64.to_str(diff)} ${I64.to_str(prod)} ${F64.to_str(quot)}")
    Ok({})
}
