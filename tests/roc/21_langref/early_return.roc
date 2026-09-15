# Syntax: `return` leaves the enclosing FUNCTION immediately.
#
# Distinct from `break`, which leaves the nearest loop.
app [main!] {}

sign = |n| {
    if n < 0 { return "neg" } else { {} }
    if n == 0 { return "zero" } else { {} }
    "pos"
}

main! = |_args| {
    echo!("${sign(0 - 1)},${sign(0)},${sign(1)}")
    Ok({})
}
