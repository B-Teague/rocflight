# Syntax: bare tags — a tag with no payload is a value, not a call.
app [main!] {}

favourite = Green

main! = |_args| {
    echo!("${Str.inspect(favourite)},${Str.inspect(Red)}")
    Ok({})
}
