# Syntax: List.len — note it returns U64, not I64.
app [main!] {}

size = |xs| List.len(xs)

main! = |_args| {
    echo!("${U64.to_str(size([1, 2, 3]))},${U64.to_str(size([]))}")
    Ok({})
}
