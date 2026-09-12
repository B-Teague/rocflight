//! Phase 4 Tests: Binary Operators
//!
//! Tests for arithmetic, comparison, and logical operators

#[cfg(test)]
mod phase4_tests {
    use rocflight::parser::Parser;
    use rocflight::types::TypeChecker;
    use rocflight::eval::Evaluator;
    use rocflight::eval::Value;

    // Arithmetic operators
    #[test]
    fn test_parse_addition() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        assert!(matches!(expr, rocflight::ast::Expr::BinOp { .. }));
    }

    #[test]
    fn test_eval_addition_int() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 3);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_addition_float() {
        let mut parser = Parser::new("1.5 + 2.5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Float(f) = result {
            assert!((f - 4.0).abs() < 0.0001);
        } else {
            panic!("Expected float result");
        }
    }

    #[test]
    fn test_eval_subtraction() {
        let mut parser = Parser::new("10 - 3");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 7);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_multiplication() {
        let mut parser = Parser::new("6 * 7");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 42);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_division() {
        let mut parser = Parser::new("10 / 2");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 5);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_string_concatenation() {
        let mut parser = Parser::new("\"hello\" + \"world\"");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Str(s) = result {
            assert_eq!(s, "helloworld");
        } else {
            panic!("Expected string result");
        }
    }

    // Comparison operators
    #[test]
    fn test_eval_equality_true() {
        let mut parser = Parser::new("5 == 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_equality_false() {
        let mut parser = Parser::new("5 == 6");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 0); // False
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_not_equal() {
        let mut parser = Parser::new("5 != 6");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_less_than() {
        let mut parser = Parser::new("3 < 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_less_than_false() {
        let mut parser = Parser::new("5 < 3");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 0); // False
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_less_than_or_equal() {
        let mut parser = Parser::new("5 <= 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_greater_than() {
        let mut parser = Parser::new("7 > 3");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_greater_than_or_equal() {
        let mut parser = Parser::new("5 >= 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    // Logical operators
    #[test]
    fn test_eval_logical_and_true() {
        let mut parser = Parser::new("1 && 1");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_logical_and_false() {
        let mut parser = Parser::new("1 && 0");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 0); // False
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_logical_or_true() {
        let mut parser = Parser::new("0 || 1");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_eval_logical_or_false() {
        let mut parser = Parser::new("0 || 0");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 0); // False
        } else {
            panic!("Expected int result");
        }
    }

    // Operator precedence
    #[test]
    fn test_precedence_multiplication_before_addition() {
        let mut parser = Parser::new("2 + 3 * 4");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 14); // 2 + (3 * 4) = 14, not (2 + 3) * 4 = 20
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_precedence_comparison_after_arithmetic() {
        let mut parser = Parser::new("2 + 3 > 4");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // (2 + 3) > 4 = 5 > 4 = true
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_precedence_logical_after_comparison() {
        let mut parser = Parser::new("2 < 3 && 4 < 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // (2 < 3) && (4 < 5) = true && true = true
        } else {
            panic!("Expected int result");
        }
    }

    // Mixed int/float operations
    #[test]
    fn test_eval_mixed_int_float_addition() {
        let mut parser = Parser::new("5 + 2.5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Float(f) = result {
            assert!((f - 7.5).abs() < 0.0001);
        } else {
            panic!("Expected float result");
        }
    }

    #[test]
    fn test_eval_mixed_int_float_comparison() {
        let mut parser = Parser::new("5 < 5.1");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    // Complex expressions with let bindings
    #[test]
    fn test_binop_with_let_binding() {
        let source = r#"let x = 5 in let y = 3 in x + y"#;
        let mut parser = Parser::new(source);
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 8);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_binop_with_lambda_and_call() {
        let source = r#"let add = |x| |y| x + y in add(3)(4)"#;
        let mut parser = Parser::new(source);
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 7);
        } else {
            panic!("Expected int result");
        }
    }

    // Type checking
    #[test]
    fn test_type_check_arithmetic() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        let mut type_checker = TypeChecker::new();
        let result = type_checker.synth(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_comparison() {
        let mut parser = Parser::new("5 < 10");
        let expr = parser.parse_expr().unwrap();
        let mut type_checker = TypeChecker::new();
        let result = type_checker.synth(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_logical() {
        let mut parser = Parser::new("1 && 0");
        let expr = parser.parse_expr().unwrap();
        let mut type_checker = TypeChecker::new();
        let result = type_checker.synth(&expr);
        assert!(result.is_ok());
    }

    // Division by zero
    #[test]
    fn test_division_by_zero_error() {
        let mut parser = Parser::new("10 / 0");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.message.contains("Division by zero"));
        }
    }

    // Operator chaining
    #[test]
    fn test_chained_arithmetic() {
        let mut parser = Parser::new("2 + 3 + 4");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 9); // (2 + 3) + 4 = 9
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_chained_comparison() {
        let mut parser = Parser::new("1 < 2 && 2 < 3 && 3 < 4");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // All true
        } else {
            panic!("Expected int result");
        }
    }

    // Negative literals
    #[test]
    fn test_negative_literal() {
        let mut parser = Parser::new("-5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, -5);
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_negative_literal_with_subtraction() {
        let mut parser = Parser::new("10 - 5");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 5);
        } else {
            panic!("Expected int result");
        }
    }

    // String equality
    #[test]
    fn test_string_equality() {
        let mut parser = Parser::new("\"hello\" == \"hello\"");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }

    #[test]
    fn test_string_inequality() {
        let mut parser = Parser::new("\"hello\" != \"world\"");
        let expr = parser.parse_expr().unwrap();
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval(&expr).unwrap();
        if let Value::Int(n) = result {
            assert_eq!(n, 1); // True
        } else {
            panic!("Expected int result");
        }
    }
}
