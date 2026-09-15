# Syntax: tuple literals. Elements may differ in type, unlike a list.
#
# `(1)` is NOT a one-tuple — it is just a parenthesised expression.
app [main!] {}

pair = ("Roc", 1)

tri = (1, 2, 3)

nested = (1, ("a", 2))

main! = |_args| {
    echo!("${Str.inspect(pair)},${Str.inspect(tri)},${Str.inspect(nested)},${I64.to_str(pair.1)},${I64.to_str(tri.0 + tri.1 + tri.2)},${I64.to_str(nested.0)},${I64.to_str(nested.1.1)}")
    Ok({})
}
