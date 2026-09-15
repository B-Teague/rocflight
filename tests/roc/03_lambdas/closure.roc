# Syntax: closure capturing an outer binding
app [main!] {}

make_adder = |n| |x| x + n

main! = |_args| {
    add5 = make_adder(5)
    echo!(I64.to_str(add5(37)))
    Ok({})
}
