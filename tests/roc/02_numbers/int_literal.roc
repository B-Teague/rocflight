# Syntax: decimal integer literal (incl. negative)
app [main!] {}

birds = 3
debt = -7

main! = |_args| {
    echo!("${I64.to_str(birds)} ${I64.to_str(debt)}")
    Ok({})
}
