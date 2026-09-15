# Syntax: `_` digit separators, allowed anywhere inside a number.
#
# A separator must sit BETWEEN digits, so it never starts or ends a run.
app [main!] {}

main! = |_args| {
    big = 1_000_000
    hex = 0xFF_FF
    bin = 0b1010_1010
    frac = 1_0.2_5
    echo!("${I64.to_str(big)},${I64.to_str(hex)},${I64.to_str(bin)},${F64.to_str(frac)}")
    Ok({})
}
