# Syntax: static dispatch — `receiver.method(args)` resolves via the receiver's TYPE.
#
# `n.to_str()` is `I64.to_str(n)`. The receiver becomes the first argument, which is
# why roc's builtins take their subject first.
#
# The binding is annotated on purpose: an unconstrained integer literal defaults to a
# fractional type, so `42.to_str()` would print "42.0".
app [main!] {}

main! = |_args| {
    n = I64.from_str("42") ?? 0
    small = I64.from_str("7") ?? 0
    echo!("${n.to_str()},${small.to_str()}")
    Ok({})
}
