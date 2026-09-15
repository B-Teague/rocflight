# Syntax: `?` error-propagation operator
app [main!] {}

# The signature below STAYS because roc refuses the file without it: unannotated,
# roc sees through the function, finds the match value known at compile time, and
# warns — and `roc check` exits non-zero on a warning. Everywhere else in tests/roc
# a sugared file carries no annotations — types are inferred.
parse_both : Str, Str => Try(I64, [BadNumStr])
parse_both = |a, b| {
    x = I64.from_str(a)?
    y = I64.from_str(b)?
    Ok(x + y)
}

main! = |_args| {
    match parse_both("1", "2") {
        Ok(n) => echo!(I64.to_str(n))
        Err(_) => echo!("err")
    }
    Ok({})
}
