# Syntax: a type variable inside a container — `List(a) -> List(a)`.
#
# `count` returns U64 because that is what `List.len` gives; annotating it I64 makes
# roc reject the definition.
app [main!] {}

# The signature below STAYS: it is the syntax under test. Everywhere else in
# tests/roc a sugared file carries no annotations — types are inferred.
echo_list : List(a) -> List(a)
echo_list = |xs| xs

count : List(a) -> U64
count = |xs| List.len(xs)

main! = |_args| {
    nums = echo_list([1, 2])
    words = echo_list(["a", "b", "c"])
    echo!("${Str.inspect(nums)},${Str.inspect(words)},${U64.to_str(count(words))},${I64.to_str(nums.fold(0, |a, x| a + x))}")
    Ok({})
}
