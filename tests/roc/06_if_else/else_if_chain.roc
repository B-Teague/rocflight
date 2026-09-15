# Syntax: else-if chain, and a multi-line if with no braces.
app [main!] {}

name_of =
    |n|
        if n == 3
            "Three"
        else if n == 4
            "Four"
        else
            "Other"

main! = |_args| {
    echo!("${name_of(3)},${name_of(4)},${name_of(9)}")
    Ok({})
}
