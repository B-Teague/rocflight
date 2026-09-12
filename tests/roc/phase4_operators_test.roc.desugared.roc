# Phase 4: Binary Operators (Arithmetic, Comparison, Logical)
# Test file for verifying operator support
#
# Run with: roc run tests/roc/phase4_operators_test.roc
#
# Type Verification (from roc repl):
# 1 + 2 : I64
# 5 < 10 : Bool
# a && b : Bool where a : Bool, b : Bool

main = |_args| {
    echo("=== Phase 4: Binary Operators ===\n")

    # Arithmetic Operators
    echo("--- Arithmetic ---\n")

    test_add : I64
    test_add = 5 + 3
    echo("5 + 3 = ${Num.to_str(test_add)}\n")
    expect test_add == 8

    test_sub : I64
    test_sub = 10 - 4
    echo("10 - 4 = ${Num.to_str(test_sub)}\n")
    expect test_sub == 6

    test_mul : I64
    test_mul = 6 * 7
    echo("6 * 7 = ${Num.to_str(test_mul)}\n")
    expect test_mul == 42

    test_div : F64
    test_div = 10.0 / 2.0
    echo("10.0 / 2.0 = ${Num.to_str(test_div)}\n")
    expect test_div == 5.0

    # Operator Precedence
    echo("--- Precedence ---\n")

    precedence_test : I64
    precedence_test = 2 + 3 * 4  # Should be 14, not 20
    echo("2 + 3 * 4 = ${Num.to_str(precedence_test)} (expects 14)\n")
    expect precedence_test == 14

    # Comparison Operators
    echo("--- Comparison ---\n")

    test_eq : Bool
    test_eq = 5 == 5
    echo("5 == 5: ${Str.inspect(test_eq)}\n")
    expect test_eq == Bool.True

    test_ne : Bool
    test_ne = 5 != 6
    echo("5 != 6: ${Str.inspect(test_ne)}\n")
    expect test_ne == Bool.True

    test_lt : Bool
    test_lt = 3 < 5
    echo("3 < 5: ${Str.inspect(test_lt)}\n")
    expect test_lt == Bool.True

    test_le : Bool
    test_le = 5 <= 5
    echo("5 <= 5: ${Str.inspect(test_le)}\n")
    expect test_le == Bool.True

    test_gt : Bool
    test_gt = 7 > 3
    echo("7 > 3: ${Str.inspect(test_gt)}\n")
    expect test_gt == Bool.True

    test_ge : Bool
    test_ge = 5 >= 5
    echo("5 >= 5: ${Str.inspect(test_ge)}\n")
    expect test_ge == Bool.True

    # Logical Operators
    echo("--- Logical ---\n")

    test_and_true : Bool
    test_and_true = Bool.True && Bool.True
    echo("True && True: ${Str.inspect(test_and_true)}\n")
    expect test_and_true == Bool.True

    test_and_false : Bool
    test_and_false = Bool.True && Bool.False
    echo("True && False: ${Str.inspect(test_and_false)}\n")
    expect test_and_false == Bool.False

    test_or_true : Bool
    test_or_true = Bool.False || Bool.True
    echo("False || True: ${Str.inspect(test_or_true)}\n")
    expect test_or_true == Bool.True

    test_or_false : Bool
    test_or_false = Bool.False || Bool.False
    echo("False || False: ${Str.inspect(test_or_false)}\n")
    expect test_or_false == Bool.False

    # Complex Expression
    echo("--- Complex ---\n")

    complex : Bool
    complex = 5 < 10 && 10 < 20
    echo("5 < 10 && 10 < 20: ${Str.inspect(complex)}\n")
    expect complex == Bool.True

    # String Concatenation
    echo("--- String Concat ---\n")

    test_str_concat : Str
    test_str_concat = "Hello" + " " + "World"
    echo("\"Hello\" + \" \" + \"World\" = ${test_str_concat}\n")
    expect test_str_concat == "Hello World"

    echo("\n✅ Phase 4: All operator tests passed!\n")
    Ok({})
}

# Type checks:
# 5 + 3 : I64
# 5 == 5 : Bool
# Bool.True && Bool.False : Bool
# "Hello" + "World" : Str
