# Syntax: one-line if/else — no parens around the condition, no braces.
# Every if MUST have an else: roc rejects a bare if.
app [main!] {}

classify = |n| if n == 1 "One" else "NotOne"

main! = |_args| {
    echo!("${classify(1)},${classify(2)}")
    Ok({})
}
