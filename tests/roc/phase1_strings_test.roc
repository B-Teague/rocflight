# Phase 1: String Literals & Interpolation
# Test file for verifying Roc string support
#
# Run with: roc run tests/roc/phase1_strings_test.roc
#
# Type Verification (from roc repl):
# "hello" : Str
# "Value: ${1 + 2}" : Str
# Str.concat("a", "b") : Str

main! = |_args| {
    # Test 1: Simple string literal
    test1 : Str
    test1 = "hello"
    echo!("Test 1 - Simple string: ${test1}\n")
    expect test1 == "hello"

    # Test 2: String with escape sequences
    test2 : Str
    test2 = "line1\nline2\ttabbed"
    echo!("Test 2 - Escaped string:\n${test2}\n")
    expect test2 == "line1\nline2\ttabbed"

    # Test 3: String interpolation with variable
    name : Str
    name = "World"
    test3 : Str
    test3 = "Hello, ${name}!"
    echo!("Test 3 - Interpolation: ${test3}\n")
    expect test3 == "Hello, World!"

    # Test 4: String interpolation with expression
    test4 : Str
    test4 = "Two plus two is ${Num.to_str(2 + 2)}"
    echo!("Test 4 - Expression interpolation: ${test4}\n")
    expect test4 == "Two plus two is 4"

    # Test 5: Empty string
    test5 : Str
    test5 = ""
    echo!("Test 5 - Empty string length: ${Num.to_str(Str.len(test5))}\n")
    expect test5 == ""
    expect Str.len(test5) == 0

    # Test 6: Quoted characters
    test6 : Str
    test6 = "quote: \"example\""
    echo!("Test 6 - Quoted string: ${test6}\n")
    expect test6 == "quote: \"example\""

    # Test 7: Backslash in string
    test7 : Str
    test7 = "path: C:\\Users\\name"
    echo!("Test 7 - Backslash: ${test7}\n")
    expect test7 == "path: C:\\Users\\name"

    echo!("\n✅ Phase 1: All string tests passed!\n")
    Ok({})
}

# Type checks (these are comments but show expected types):
# test1 : Str = "hello"
# test3 : Str = "Hello, ${name}!" where name : Str = "World"
# test4 : Str = "Two plus two is ${Num.to_str(2 + 2)}"
