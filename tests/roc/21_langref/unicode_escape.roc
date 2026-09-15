# Syntax: `\u(hex)` inserts a code point by its hexadecimal value.
app [main!] {}

main! = |_args| {
    accented = "caf\u(e9)"
    combining = "cafe\u(301)"
    echo!("${accented},${combining}")
    Ok({})
}
