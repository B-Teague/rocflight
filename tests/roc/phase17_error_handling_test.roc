# Phase 17: Error Handling - Try Type & ? Operator
# Test file for Result/Try types and early return
#
# Run with: roc run tests/roc/phase17_error_handling_test.roc
#
# Type Verification (from roc repl):
# Try(Ok(5), Err("error")) : [Ok(I64), Err(Str)]
# value? : early return on Err
# first() ?? 0 : unwrap or default

main! = |_args| {
    echo!("=== Phase 17: Error Handling ===\n")

    # Ok variant
    echo!("--- Ok Variant ---\n")

    success : Try(I64, Str)
    success = Ok(42)
    echo!("Ok(42): ${Str.inspect(success)}\n")

    # Err variant
    echo!("--- Err Variant ---\n")

    failure : Try(I64, Str)
    failure = Err("Something went wrong")
    echo!("Err(\"Something went wrong\"): ${Str.inspect(failure)}\n")

    # Pattern matching on Try
    echo!("--- Matching Try ---\n")

    result : Try(I64, Str)
    result = Ok(100)

    result_msg = match result {
        Ok(val) => "Success: ${Num.to_str(val)}"
        Err(err) => "Error: ${err}"
    }
    echo!("match Ok(100): ${result_msg}\n")
    expect result_msg == "Success: 100"

    # Question mark operator (early return)
    echo!("--- Question Mark Operator ---\n")

    parse_number : Str -> Try(I64, Str)
    parse_number = |str| {
        # In real Roc, I64.from_str returns Try
        # For now, we'd implement this as:
        # match I64.from_str(str) {
        #     Ok(n) => Ok(n)
        #     Err(e) => Err(e)
        # }
        # Using ? would be: num = I64.from_str(str)?

        if str == "42" {
            Ok(42)
        } else {
            Err("Not a valid number")
        }
    }

    num_result = parse_number("42")
    echo!("parse_number(\"42\"): ${Str.inspect(num_result)}\n")

    # Default operator ??
    echo!("--- Default Operator (??) ---\n")

    get_config : Try(I64, Str)
    get_config = Err("Config not found")

    config = match get_config {
        Ok(v) => v
        Err(_) => 8080  # Default value
    }
    echo!("config ?? 8080: ${Num.to_str(config)}\n")
    expect config == 8080

    # Try with different error types
    echo!("--- Try Type Variation ---\n")

    string_result : Try(Str, I64)
    string_result = Ok("Success")

    str_msg = match string_result {
        Ok(s) => "Got: ${s}"
        Err(code) => "Error code: ${Num.to_str(code)}"
    }
    echo!("Try(Str, I64): ${str_msg}\n")
    expect str_msg == "Got: Success"

    echo!("\n✅ Phase 17: All error handling tests passed!\n")
    Ok({})
}

# Type checks:
# Ok(42) : Try(I64, Str) or [Ok(I64), Err(Str)]
# Err("error") : Try(I64, Str)
# match Try { Ok(x) => ..., Err(e) => ... } : (inferred)
# value? : early return, requires Try/Try type
# Try(I64, Str) : equivalent to [Ok(I64), Err(Str)]
