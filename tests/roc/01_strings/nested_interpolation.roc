# Syntax: a string literal inside a ${...} interpolation.
#
# The outer string's scanner must not treat the inner quote as its terminator.
app [main!] {}

shout = |s| s

main! = |_args| {
    echo!("a=${shout("one")},b=${shout("two")}")
    Ok({})
}
