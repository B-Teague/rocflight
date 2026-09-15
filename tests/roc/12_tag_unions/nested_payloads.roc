# Syntax: tags nested in tags, and tags carrying records.
app [main!] {}

nested = Outer(Inner("y"))

mixed = Wrap({ b: Bool.True, a: Red })

main! = |_args| {
    echo!("${Str.inspect(nested)}|${Str.inspect(mixed)}")
    Ok({})
}
