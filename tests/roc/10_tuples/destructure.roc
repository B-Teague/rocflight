# Syntax: destructuring a tuple in a binding position.
app [main!] {}

pair = ("Roc", 1)

main! = |_args| {
    (name, n) = pair
    echo!("${name},${I64.to_str(n)}")
    Ok({})
}
