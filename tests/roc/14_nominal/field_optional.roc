# Syntax: an optional field — `field ?: Type` — read with `.?field`.
#
# Unlike a defaulted field, an optional one may genuinely be absent, so reading it
# gives a `Try`: `Ok(value)` or `Err(MissingField)`.
app [main!] {}

Cfg := { host: Str, timeout ?: U64 }

describe = |c| match c.?timeout {
    Ok(t) => "${t.to_str()}ms"
    Err(MissingField) => "no timeout"
}

main! = |_args| {
    echo!("${describe(Cfg.{ host: "a" })},${describe(Cfg.{ host: "b", timeout: 30 })}")
    Ok({})
}
