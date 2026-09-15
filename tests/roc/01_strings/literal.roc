# Syntax: string literal
app [main!] {}

greeting = "hello world"

main! = |_args| {
    echo!(greeting)
    Ok({})
}
