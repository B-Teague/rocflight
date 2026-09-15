# Syntax: alternative patterns joined with |
app [main!] {}

warm_or_cool = |c| match c {
    Red | Orange => "warm"
    Blue | Cyan => "cool"
}

main! = |_args| {
    echo!("${warm_or_cool(Red)},${warm_or_cool(Orange)},${warm_or_cool(Cyan)}")
    Ok({})
}
