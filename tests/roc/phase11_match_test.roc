# Phase 11: Pattern Matching (Critical Phase)
# Test file for match expressions and pattern matching
#
# Run with: roc run tests/roc/phase11_match_test.roc
#
# Type Verification (from roc repl):
# match x { pattern => expr } : (inferred from branches)
# [Red, Green, Blue] : tag union type

main! = |_args| {
    echo!("=== Phase 11: Pattern Matching ===\n")

    # Simple literal matching
    echo!("--- Literal Patterns ---\n")

    test_num = 5
    result_num = match test_num {
        5 => "Five"
        _ => "Other"
    }
    echo!("match 5: ${result_num}\n")
    expect result_num == "Five"

    # Tag union matching
    echo!("--- Tag Matching ---\n")

    Color := [Red, Green, Blue]

    test_color : Color
    test_color = Red

    color_name = match test_color {
        Red => "Red"
        Green => "Green"
        Blue => "Blue"
    }
    echo!("match Red: ${color_name}\n")
    expect color_name == "Red"

    # Tag with payload
    echo!("--- Tagged Union with Payload ---\n")

    Message := [Text(Str), Number(I64)]

    msg1 : Message
    msg1 = Text("Hello")

    msg_display = match msg1 {
        Text(t) => "Text: ${t}"
        Number(n) => "Number: ${Num.to_str(n)}"
    }
    echo!("match Text(\"Hello\"): ${msg_display}\n")
    expect msg_display == "Text: Hello"

    # Wildcard pattern
    echo!("--- Wildcard Pattern ---\n")

    x : I64
    x = 100

    wildcard_result = match x {
        0 => "Zero"
        _ => "Non-zero"
    }
    echo!("match 100 (wildcard): ${wildcard_result}\n")
    expect wildcard_result == "Non-zero"

    # Multiple payloads
    echo!("--- Multiple Payloads ---\n")

    Data := [Pair(I64, I64), Single(I64)]

    pair_data : Data
    pair_data = Pair(3, 4)

    pair_result = match pair_data {
        Pair(a, b) => Num.to_str(a + b)
        Single(x) => Num.to_str(x)
    }
    echo!("match Pair(3, 4): sum = ${pair_result}\n")
    expect pair_result == "7"

    # Guard clauses
    echo!("--- Guard Clauses ---\n")

    guard_num = 15

    guard_result = match guard_num {
        x if x > 10 => "Greater than 10"
        x if x > 0 => "Positive but <= 10"
        _ => "Zero or negative"
    }
    echo!("match 15 (with guard): ${guard_result}\n")
    expect guard_result == "Greater than 10"

    echo!("\n✅ Phase 11: All pattern matching tests passed!\n")
    Ok({})
}

# Type checks:
# Red : [Red, Green, Blue]
# match Red { Red => "Red", _ => "Other" } : Str
# Text("hello") : [Text(Str), Number(I64)]
# Pair(3, 4) : [Pair(I64, I64), Single(I64)]
