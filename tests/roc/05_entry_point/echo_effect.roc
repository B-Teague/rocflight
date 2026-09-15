# Syntax: effectful call `echo!(...)`.
# `echo!` is provided by the default (platformless) host, NOT a global builtin:
# it is out of scope in a type module. Verified signature: Str => {}
app [main!] {}

shout = |s| "${s}!"

main! = |_args| {
    echo!(shout("one"))
    echo!(shout("two"))
    Ok({})
}
