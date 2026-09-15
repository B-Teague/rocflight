# Syntax: and / or keywords, ! prefix negation
app [main!] {}

main! = |_args| {
    r = { both: Bool.True and Bool.False, either: Bool.True or Bool.False, negated: !Bool.True }
    echo!(Str.inspect(r))
    Ok({})
}
