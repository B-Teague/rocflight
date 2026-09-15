# Syntax: record literal { field: value }, including a trailing comma
app [main!] {}

point = { x: Bool.True, y: Bool.False, }

main! = |_args| {
    echo!(Str.inspect(point))
    Ok({})
}
