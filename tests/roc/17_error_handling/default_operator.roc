# Syntax: `??` supplies a value to use when the left side is Err.
#
# The parse goes through a function so the input is not a compile-time constant:
# roc warns "this match value is known at compile time" on a constant scrutinee.
app [main!] {}

parse_or = |s, d| I64.from_str(s) ?? d

main! = |_args| {
    echo!("${I64.to_str(parse_or("42", 0))},${I64.to_str(parse_or("nope", 7))}")
    Ok({})
}
