# Syntax: the SAME variable name used twice ties those positions together.
#
# `pair : a, a -> a` requires both arguments to have one type; `first : a, b -> a`
# lets them differ.
app [main!] {}

# The signature below STAYS: it is the syntax under test. Everywhere else in
# tests/roc a sugared file carries no annotations — types are inferred.
pair : a, a -> a
pair = |x, _y| x

first : a, b -> a
first = |x, _y| x

main! = |_args| {
    both = pair(1, 2)
    mixed = first(3, "ignored")
    echo!("${I64.to_str(both)},${I64.to_str(mixed)}")
    Ok({})
}
