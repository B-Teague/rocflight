# Syntax: record field access, including a nested chain
app [main!] {}

nested = { inner: { depth: 2 }, top: 1 }

main! = |_args| {
    echo!("${I64.to_str(nested.top)},${I64.to_str(nested.inner.depth)}")
    Ok({})
}
