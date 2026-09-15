# Syntax: a tag pattern binding its payloads.
app [main!] {}

render = |t| match t {
    Foo(n, label) => "${label}=${I64.to_str(n)}"
    Bar => "bar"
}

main! = |_args| {
    echo!("${render(Foo(42, "answer"))},${render(Bar)}")
    Ok({})
}
