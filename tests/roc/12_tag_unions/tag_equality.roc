# Syntax: comparing tags with == and !=
app [main!] {}

chosen = Green

main! = |_args| {
    echo!(Str.inspect({ same: chosen == Green, other: chosen == Red }))
    Ok({})
}
