# Syntax: a type variable — `a` is a lowercase name in a signature, meaning "any type".
#
# The same function is used at TWO different types below. That only works because each
# use gets its own instance of `a`; otherwise the first use would pin it.
app [main!] {}

# The signature below STAYS: it is the syntax under test. Everywhere else in
# tests/roc a sugared file carries no annotations — types are inferred.
identity : a -> a
identity = |x| x

main! = |_args| {
    s = identity("hi")
    n = identity(5)
    echo!("${s},${I64.to_str(n)}")
    Ok({})
}
