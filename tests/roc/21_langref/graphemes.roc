# Syntax: grapheme literals — `'a'` is a NUMBER, not a one-character Str.
#
# roc has no character type. `'a'` is a number literal spelled visually, so it
# unifies with any number type and takes part in arithmetic.
app [main!] {}

main! = |_args| {
    a = 'a'
    next = 'a' + 1
    accented = 'é'
    newline = '\n'
    escaped = '\u(e9)'
    echo!("${I64.to_str(a)},${I64.to_str(next)},${I64.to_str(accented)},${I64.to_str(newline)},${I64.to_str(escaped)}")
    Ok({})
}
