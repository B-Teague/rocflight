# Syntax: `for x in list { ... }` with a mutable `var`.
#
# `var` declares a rebindable name; a plain binding cannot be reassigned, roc reports
# it as a redeclaration. The `$` is part of the identifier, like the `!` on an
# effectful name — `$sum` and `sum` are different names.
app [main!] {}

total = |xs| {
    var $sum = 0
    for n in xs {
        $sum = $sum + n
    }
    $sum
}

main! = |_args| {
    echo!("${I64.to_str(total([1, 2, 3, 4]))},${I64.to_str(total([]))}")
    Ok({})
}
