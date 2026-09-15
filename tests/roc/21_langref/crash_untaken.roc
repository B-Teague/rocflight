# Syntax: `crash` aborts the program.
#
# The branch here is never taken — a crash that IS taken has no output to compare,
# and with a constant condition roc evaluates it at compile time and refuses the file.
app [main!] {}

check = |args| if List.len(args) > 99 { crash "impossible" } else { "ok" }

main! = |args| {
    echo!(check(args))
    Ok({})
}
