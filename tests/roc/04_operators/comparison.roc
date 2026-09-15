# Syntax: == != < <= > >=
app [main!] {}

main! = |_args| {
    r = { eq: 1 == 1, ne: 1 != 2, lt: 1 < 2, le: 2 <= 2, gt: 3 > 2, ge: 3 >= 3 }
    echo!(Str.inspect(r))
    Ok({})
}
