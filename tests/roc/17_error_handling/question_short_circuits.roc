# Syntax: `?` short-circuits — the rest of the block is skipped on Err.
app [main!] {}

# The signature below STAYS because roc refuses the file without it: unannotated, roc
# folds the call and warns that the match is constant. Everywhere else in tests/roc a
# sugared file carries no annotations — types are inferred.
add_one : Str => Try(I64, [BadNumStr])
add_one = |s| {
    n = I64.from_str(s)?
    Ok(n + 1)
}

main! = |_args| {
    good = match add_one("41") { Ok(v) => I64.to_str(v) Err(_) => "err" }
    bad = match add_one("nope") { Ok(v) => I64.to_str(v) Err(_) => "err" }
    echo!("${good},${bad}")
    Ok({})
}
