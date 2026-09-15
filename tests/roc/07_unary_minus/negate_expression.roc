# Syntax: unary minus applies to a whole postfix expression, not just an atom.
#
# `-r.v` is `-(r.v)` and `-scale(2)` is `-(scale(2))`.
#
# `sum` is annotated because an unconstrained literal defaults to a fractional type,
# so `-(1 + 2)` would render as "-3.0".
app [main!] {}

scale = |x| x * 10

main! = |_args| {
    r = { v: 4 }
    sum = 1 + 2
    echo!("${(I64.to_str(-r.v))},${(I64.to_str(-scale(2)))},${(I64.to_str(-sum))}")
    Ok({})
}
