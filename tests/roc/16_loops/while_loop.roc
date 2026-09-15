# Syntax: `while cond { ... }`.
app [main!] {}

sum_below = |limit| {
    var $i = 0
    var $sum = 0
    while $i < limit {
        $sum = $sum + $i
        $i = $i + 1
    }
    $sum
}

main! = |_args| {
    echo!("${I64.to_str(sum_below(5))},${I64.to_str(sum_below(0))}")
    Ok({})
}
