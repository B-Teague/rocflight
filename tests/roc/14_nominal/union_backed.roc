# Syntax: a nominal type over a tag union — `Name := [ ... ]`.
#
# Tags are reached as `Name.Tag`, in both construction and patterns.
app [main!] {}

Animal := [Dog(Str), Cat(Str)]

speak = |a| match a {
    Animal.Dog(name) => "${name} says woof"
    Animal.Cat(name) => "${name} says meow"
}

main! = |_args| {
    echo!("${speak(Animal.Dog("rex"))},${speak(Animal.Cat("tom"))}")
    Ok({})
}
