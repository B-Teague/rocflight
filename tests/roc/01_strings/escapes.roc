# Syntax: escape sequences \n \t \" \\
app [main!] {}

line = "tab:\there\nquote:\"q\"\nbackslash:\\"

main! = |_args| {
    echo!(line)
    Ok({})
}
