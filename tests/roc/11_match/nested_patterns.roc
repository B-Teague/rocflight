# Syntax: a tag pattern nested inside another tag pattern.
app [main!] {}

unwrap = |t| match t {
    Wrap(Inner(s)) => s
    Wrap(Empty) => "empty"
    Bare => "bare"
}

main! = |_args| {
    echo!("${unwrap(Wrap(Inner("deep")))},${unwrap(Wrap(Empty))},${unwrap(Bare)}")
    Ok({})
}
