# Syntax: a nominal type over a record — `Name := { ... }`.
#
# Constructed with `Name.{ ... }`; fields are read with plain `.field`.
app [main!] {}

Point := { x: I64, y: I64 }

origin_distance = |p| p.x + p.y

main! = |_args| {
    here = Point.{ x: 3, y: 4 }
    echo!("${I64.to_str(origin_distance(here))},${Str.inspect(here)}")
    Ok({})
}
