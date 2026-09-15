# Syntax: a guard on a pattern — `pattern if condition => body`
app [main!] {}

sign_of = |n| match n {
    0 => "zero"
    x if x < 0 => "negative"
    _ => "positive"
}

main! = |_args| {
    echo!("${sign_of(0)},${sign_of(0 - 5)},${sign_of(5)}")
    Ok({})
}
