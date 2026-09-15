# Syntax: // truncating division, % remainder
app [main!] {}

main! = |_args| {
    d = 7 // 2
    m = 7 % 2
    echo!("${I64.to_str(d)} ${I64.to_str(m)}")
    Ok({})
}
