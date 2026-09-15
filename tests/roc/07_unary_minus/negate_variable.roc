# Syntax: unary minus on a variable.
#
# Distinct from binary `-`: this takes one operand, not two.
app [main!] {}

main! = |_args| {
    n = 5
    echo!(I64.to_str(-n))
    Ok({})
}
