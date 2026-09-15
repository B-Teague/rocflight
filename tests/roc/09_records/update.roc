# Syntax: record update — `{ ..base, field: value }`.
#
# NOT `{ base & field: value }`: roc rejects that outright. Named fields override the
# base's; every other field is carried over.
app [main!] {}

Person : { name: Str, age: I64 }

birthday = |p| { ..p, age: 31 }

rename = |p| { ..p, name: "renamed" }

main! = |_args| {
    start = { name: "ada", age: 30 }
    echo!("${Str.inspect(birthday(start))},${Str.inspect(rename(start))},${I64.to_str(start.age)}")
    Ok({})
}
