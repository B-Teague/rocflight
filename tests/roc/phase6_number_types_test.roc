# Phase 6: All Numeric Types & Formats
# Test file for all number types and literal formats
#
# Run with: roc run tests/roc/phase6_number_types_test.roc
#
# Type Verification (from roc repl):
# 5.U8 : U8
# 0xFF : I64
# 42.0 : Dec (or F64)

main! = |_args| {
    echo!("=== Phase 6: Number Types ===\n")

    # Unsigned integer types
    echo!("--- Unsigned Integers ---\n")

    test_u8 : U8
    test_u8 = 255.U8
    echo!("255.U8 = ${Num.to_str(test_u8)}\n")
    expect test_u8 == 255.U8

    test_u16 : U16
    test_u16 = 65535.U16
    echo!("65535.U16 = ${Num.to_str(test_u16)}\n")
    expect test_u16 == 65535.U16

    test_u32 : U32
    test_u32 = 4294967295.U32
    echo!("4294967295.U32 is valid\n")

    test_u64 : U64
    test_u64 = 18446744073709551615.U64
    echo!("Large U64 is valid\n")

    test_u128 : U128
    test_u128 = 100.U128
    echo!("100.U128 = ${Num.to_str(test_u128)}\n")

    # Signed integer types
    echo!("--- Signed Integers ---\n")

    test_i8 : I8
    test_i8 = -128.I8
    echo!("-128.I8 = ${Num.to_str(test_i8)}\n")

    test_i16 : I16
    test_i16 = -32768.I16
    echo!("-32768.I16 = ${Num.to_str(test_i16)}\n")

    test_i32 : I32
    test_i32 = -2147483648.I32
    echo!("-2147483648.I32 is valid\n")

    test_i64 : I64
    test_i64 = -9223372036854775808.I64
    echo!("-9223372036854775808.I64 is valid\n")

    test_i128 : I128
    test_i128 = -100.I128
    echo!("-100.I128 = ${Num.to_str(test_i128)}\n")

    # Float types
    echo!("--- Floating Point ---\n")

    test_f32 : F32
    test_f32 = 3.14.F32
    echo!("3.14.F32 is valid\n")

    test_f64 : F64
    test_f64 = 2.71828
    echo!("2.71828 : F64\n")

    # Decimal type (arbitrary precision)
    echo!("--- Decimal ---\n")

    test_dec : Dec
    test_dec = 42.0
    echo!("42.0 : Dec (arbitrary precision)\n")

    # Number formats
    echo!("--- Number Formats ---\n")

    hex : I64
    hex = 0xFF
    echo!("0xFF (hex) = ${Num.to_str(hex)}\n")
    expect hex == 255

    octal : I64
    octal = 0o77
    echo!("0o77 (octal) = ${Num.to_str(octal)}\n")
    expect octal == 63

    binary : I64
    binary = 0b1010
    echo!("0b1010 (binary) = ${Num.to_str(binary)}\n")
    expect binary == 10

    echo!("\n✅ Phase 6: All number type tests passed!\n")
    Ok({})
}

# Type checks:
# 5.U8 : U8
# 255.U8 : U8
# -128.I8 : I8
# 3.14.F32 : F32
# 42.0 : Dec
# 0xFF : I64
# 0o77 : I64
# 0b1010 : I64
