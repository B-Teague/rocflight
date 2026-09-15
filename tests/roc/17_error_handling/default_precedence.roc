# Syntax: `??` binds looser than arithmetic, so the whole right side is the default.
app [main!] {}

parse_or_sum = |s| I64.from_str(s) ?? 1 + 2

main! = |_args| {
    echo!("${I64.to_str(parse_or_sum("nope"))},${I64.to_str(parse_or_sum("9"))}")
    Ok({})
}
