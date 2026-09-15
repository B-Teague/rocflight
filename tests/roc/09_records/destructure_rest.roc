# Syntax: `..rest` in a record pattern binds every field you did not name.
#
# Naming a field with `_` and capturing the rest is how you drop a field.
app [main!] {}

Full : { name: Str, age: I64, email: Str }
Trimmed : { name: Str, age: I64 }

drop_email = |p| {
    { email: _, ..rest } = p
    rest
}

main! = |_args| {
    trimmed = drop_email({ name: "ada", age: 30, email: "a@b.c" })
    echo!("${Str.inspect(trimmed)},${I64.to_str(trimmed.age)}")
    Ok({})
}
