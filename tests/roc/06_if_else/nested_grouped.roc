# Syntax: an if nested in a branch, grouped with parens.
#
# This only parses because function application requires NO space before `(`:
# roc rejects `f (1)`. Without that rule the condition `n > 0` would swallow the
# parenthesised then-branch as a call on `0`.
app [main!] {}

size = |n| if n > 0 (if n > 10 "big" else "small") else "neg"

main! = |_args| {
    echo!("${size(20)},${size(5)},${size(0 - 1)}")
    Ok({})
}
