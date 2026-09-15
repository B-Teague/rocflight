# Syntax: a nominal type with a method block — `Name :: backing.{ ... }`.
#
# The block holds ordinary functions, reached as `Name.method(...)`.
app [main!] {}

Secret :: { key: Str }.{
    new = |k| { key: k }

    reveal = |s| s.key
}

main! = |_args| {
    s = Secret.new("hunter2")
    echo!(Secret.reveal(s))
    Ok({})
}
