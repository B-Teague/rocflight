# Syntax: a type alias — `Name : Type` at the top level.
#
# An alias is TRANSPARENT: `Bytes` and `List(U8)` are the same type, and a value of
# one passes freely where the other is wanted. That is what separates it from a
# nominal (`:=`), which makes a genuinely distinct type.
#
# The capital letter is what tells an alias from a value annotation — a lowercase
# `x : I64` annotates the binding `x` that follows it.
app [main!] {}

Bytes : List(U8)
Label : Str

# The signature below STAYS: it is the syntax under test. Everywhere else in
# tests/roc a sugared file carries no annotations — types are inferred.
size : Bytes -> U64
size = |b| b.len()

shout : Label -> Label
shout = |l| l.concat("!")

main! = |_args| {
    raw = [65, 66, 67]
    echo!("${size(raw).to_str()},${shout("hi")}")
    Ok({})
}
