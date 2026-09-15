# Syntax: calling a method by dispatch — `value.method(args)`.
#
# Same call as `Type.method(value, args)`, with the receiver moved in front.
app [main!] {}

# The annotations in the method block STAY: without them roc cannot attach the
# methods to the type at all ("does not have a method named bump"). Everywhere
# else in tests/roc a sugared file carries no annotations — types are inferred.
Counter :: { n: I64 }.{
    start : Counter
    start = { n: 0 }

    bump : Counter, I64 -> Counter
    bump = |c, by| { ..c, n: c.n + by }

    show : Counter -> Str
    show = |c| c.n.to_str()
}

main! = |_args| {
    c = Counter.bump(Counter.start, 5)
    echo!("${c.show()},${Counter.show(c)},${c.bump(2).show()}")
    Ok({})
}
