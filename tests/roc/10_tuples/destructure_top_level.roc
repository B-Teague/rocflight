# Syntax: destructuring at the top level, not inside a block.
app [main!] {}

pair = ("Roc", 1)

(name, n) = pair

(_, second) = pair

main! = |_args| {
    echo!("${name},${I64.to_str(n)},${I64.to_str(second)}")
    Ok({})
}
