# Syntax: hex / octal / binary literals
app [main!] {}

hex = 0xFF
oct = 0o77
bin = 0b1010

main! = |_args| {
    echo!("${I64.to_str(hex)} ${I64.to_str(oct)} ${I64.to_str(bin)}")
    Ok({})
}
