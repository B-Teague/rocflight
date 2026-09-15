# Syntax: Str.inspect on a record.
# Fields come out ALPHABETICAL, not in source order — note zebra/apple/mango below.
app [main!] {}

r = { zebra: Bool.True, apple: Bool.False, mango: Bool.True }

main! = |_args| {
    echo!(Str.inspect(r))
    Ok({})
}
