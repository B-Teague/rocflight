# Syntax: scientific notation — `e` or `E`, with an optional sign.
#
# Valid with or without a fraction: `1e3` is 1000.
app [main!] {}

main! = |_args| {
    a = 1e3
    b = 1.5e3
    c = 1.5e-3
    d = 1.5E3
    echo!("${F64.to_str(a)},${F64.to_str(b)},${F64.to_str(c)},${F64.to_str(d)}")
    Ok({})
}
