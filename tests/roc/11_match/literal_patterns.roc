# Syntax: literal patterns (numbers and strings) with a wildcard catch-all.
app [main!] {}

count_name = |n| match n {
    1 => "one"
    2 => "two"
    _ => "many"
}

initial = |s| match s {
    "alice" => "A"
    "bob" => "B"
    _ => "?"
}

main! = |_args| {
    echo!("${count_name(1)},${count_name(9)},${initial("bob")},${initial("zed")}")
    Ok({})
}
