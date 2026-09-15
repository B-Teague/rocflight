# Syntax: string interpolation ${expr}
app [main!] {}

name = "Roc"
count = 3

main! = |_args| {
    echo!("${name} has ${I64.to_str(count)} letters")
    Ok({})
}
