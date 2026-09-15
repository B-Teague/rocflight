# Syntax: a union inferred from branches that yield different tags.
app [main!] {}

pick = |b| if b Red else Green

main! = |_args| {
    echo!("${Str.inspect(pick(Bool.True))},${Str.inspect(pick(Bool.False))}")
    Ok({})
}
