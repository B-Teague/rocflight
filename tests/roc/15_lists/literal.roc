# Syntax: list literals, including the empty list.
#
# Elements are annotated because an unconstrained integer literal inspects as "1.0"
# in roc — it defaults to a fractional type.
app [main!] {}

xs = [1, 2, 3]

# The annotation below STAYS because roc refuses the file without it: an empty list
# at the top level is a polymorphic value, and roc will not leave one unresolved.
# Everywhere else in tests/roc a sugared file carries no annotations — inferred.
empty : List(I64)
empty = []

main! = |_args| {
    echo!("${Str.inspect(xs)},${Str.inspect(empty)},${I64.to_str(xs.fold(0, |a, x| a + x))}")
    Ok({})
}
