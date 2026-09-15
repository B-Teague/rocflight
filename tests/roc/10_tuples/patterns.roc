# Syntax: tuple patterns in a match, with literal and binding elements.
app [main!] {}

locate = |p| match p {
    (0, 0) => "origin"
    (x, 0) => "x-axis:${I64.to_str(x)}"
    (0, y) => "y-axis:${I64.to_str(y)}"
    _ => "elsewhere"
}

main! = |_args| {
    echo!("${locate((0, 0))},${locate((3, 0))},${locate((0, 4))},${locate((1, 1))}")
    Ok({})
}
