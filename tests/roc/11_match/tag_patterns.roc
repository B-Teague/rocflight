# Syntax: match on bare tags. Arms are newline-separated; no commas needed.
app [main!] {}

describe = |c| match c {
    Red => "red"
    Green => "green"
    Blue => "blue"
}

main! = |_args| {
    echo!("${describe(Red)},${describe(Green)},${describe(Blue)}")
    Ok({})
}
