# Syntax: headerless platformless app — the `app [main!] {}` header is implied.
# This is the form `roc run <file>` accepts with no platform.
main! = |_args| {
    echo!("hello")
    Ok({})
}
