# Syntax: fractional literal
app [main!] {}

pi = 3.14
neg = -2.5

main! = |_args| {
    echo!("${F64.to_str(pi)} ${F64.to_str(neg)}")
    Ok({})
}
