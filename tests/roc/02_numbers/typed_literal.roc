# Syntax: type-suffixed numeric literal (255.U8)
app [main!] {}

small = 255.U8
wide = 42.I32

main! = |_args| {
    echo!("${U8.to_str(small)} ${I32.to_str(wide)}")
    Ok({})
}
