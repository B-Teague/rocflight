# Syntax: a lambda whose body is a record literal
app [main!] {}

flags = |on| { enabled: on, disabled: !on }

main! = |_args| {
    echo!(Str.inspect(flags(Bool.True)))
    Ok({})
}
