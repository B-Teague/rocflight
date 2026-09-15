# Syntax: `break` exits a loop early.
#
# Every `if` needs an else branch, so the non-breaking arm is `{}`.
app [main!] {}

first_negative = |xs| {
    var $found = 0
    for n in xs {
        if n < 0 {
            $found = n
            break
        } else {
            {}
        }
    }
    $found
}

main! = |_args| {
    echo!("${I64.to_str(first_negative([1, 2, 0 - 7, 3]))},${I64.to_str(first_negative([1, 2]))}")
    Ok({})
}
