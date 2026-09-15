# Syntax: `expect` asserts, `dbg` prints. Both write to STDERR and carry on.
#
# A failing `expect` is reported, not fatal — the program continues.
app [main!] {}

main! = |_args| {
    dbg "marker"
    expect 1 + 1 == 2
    expect 1 + 1 == 3
    echo!("still running")
    Ok({})
}
