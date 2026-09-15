# Syntax: a NAMED open tag union — `[Red, ..u]`.
#
# `[Red, ..]` says "Red and possibly more". Naming the rest as `..u` ties two
# positions to the same leftovers, so a function can take a union and hand back
# whatever extra tags came with it.
#
# Because the union is open, a `match` on it needs a wildcard: the compiler cannot
# know the tags it has not been shown.
app [main!] {}

# The signature below STAYS: it is the syntax under test. Everywhere else in
# tests/roc a sugared file carries no annotations — types are inferred.
describe : [Red, ..u] -> Str
describe = |c| match c {
    Red => "red"
    _ => "other"
}

main! = |_args| {
    echo!("${describe(Red)},${describe(Green)},${describe(Blue)}")
    Ok({})
}
