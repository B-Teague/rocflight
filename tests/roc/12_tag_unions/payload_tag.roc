# Syntax: tags with payloads, single and multiple.
#
# Payloads are annotated in BOTH files: an unconstrained integer literal inspects
# as "42.0" in roc (it defaults to a fractional type), so leaving it bare would
# make the two files disagree.
app [main!] {}

pair = Foo(42, "hi")

single = Wrap("x")

first_of = |t| match t {
    Foo(n, _) => n
    _ => 0
}

main! = |_args| {
    echo!("${Str.inspect(pair)},${Str.inspect(single)},${Str.inspect(Bar)},${I64.to_str(first_of(pair))}")
    Ok({})
}
