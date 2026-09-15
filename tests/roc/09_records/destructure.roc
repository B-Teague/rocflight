# Syntax: destructuring a record in a binding position.
app [main!] {}

main! = |_args| {
    person = { name: "ada", age: 30 }
    { name, age } = person
    echo!("${name},${I64.to_str(age)}")
    Ok({})
}
