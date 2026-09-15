# Syntax: `var` and reassignment outside a loop.
app [main!] {}

stepped = |start| {
    var $acc = start
    $acc = $acc * 2
    $acc = $acc + 1
    $acc
}

main! = |_args| {
    echo!("${I64.to_str(stepped(5))},${I64.to_str(stepped(0))}")
    Ok({})
}
