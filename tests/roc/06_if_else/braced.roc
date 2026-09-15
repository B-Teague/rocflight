# Syntax: if/else with braced branches. The braces are real blocks.
app [main!] {}

describe = |n| if n == 5 {
    label = "Five"
    label
} else {
    "NotFive"
}

main! = |_args| {
    echo!("${describe(5)},${describe(6)}")
    Ok({})
}
