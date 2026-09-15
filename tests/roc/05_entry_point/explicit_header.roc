# Syntax: explicit platformless app header `app [main!] {}`
app [main!] {}

main! = |_| {
    echo!("hello")
    Ok({})
}
