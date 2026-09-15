# Syntax: `Name.{ field }` as a pattern, destructuring the backing record.
app [main!] {}

Point := { x: I64, y: I64 }

get_x = |Point.{ x, y }| x + y

main! = |_args| {
    echo!(I64.to_str(get_x(Point.{ x: 9, y: 1 })))
    Ok({})
}
