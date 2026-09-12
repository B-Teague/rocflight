//! Phase 2 Tests: Numbers & Identifiers
//!
//! Tests for:
//! - Integer literals (positive and negative)
//! - Float literals
//! - Identifier parsing (Phase 2 parsing, Phase 3 evaluation)
//! - Type inference for numbers
//! - Number evaluation

#[cfg(test)]
mod phase2_tests {
    use rocflight::{Parser, TypeChecker};

    #[test]
    fn test_parse_positive_integer() {
        let mut parser = Parser::new("42");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "42");
    }

    #[test]
    fn test_parse_negative_integer() {
        let mut parser = Parser::new("-3");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "-3");
    }

    #[test]
    fn test_parse_large_integer() {
        let mut parser = Parser::new("9223372036854775807");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "9223372036854775807");
    }

    #[test]
    fn test_parse_zero() {
        let mut parser = Parser::new("0");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "0");
    }

    #[test]
    fn test_parse_float() {
        let mut parser = Parser::new("3.14");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "3.14");
    }

    #[test]
    fn test_parse_negative_float() {
        let mut parser = Parser::new("-2.5");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "-2.5");
    }

    #[test]
    fn test_parse_identifier() {
        let mut parser = Parser::new("birds");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "birds");
    }

    #[test]
    fn test_parse_identifier_with_underscore() {
        let mut parser = Parser::new("_args");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "_args");
    }

    #[test]
    fn test_parse_identifier_with_numbers() {
        let mut parser = Parser::new("var123");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "var123");
    }

    #[test]
    fn test_type_check_integer() {
        let mut parser = Parser::new("42");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut checker = TypeChecker::new();
        let ty = checker.synth(&expr).expect("Failed to infer type");

        assert_eq!(ty.to_string(), "I64");
    }

    #[test]
    fn test_type_check_float() {
        let mut parser = Parser::new("3.14");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut checker = TypeChecker::new();
        let ty = checker.synth(&expr).expect("Failed to infer type");

        assert_eq!(ty.to_string(), "F64");
    }

    #[test]
    fn test_type_check_identifier() {
        let mut parser = Parser::new("x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut checker = TypeChecker::new();
        let ty = checker.synth(&expr).expect("Failed to infer type");

        // Identifiers get fresh type variables (Phase 3 will bind them)
        assert!(ty.to_string().starts_with('$'));
    }

    #[test]
    fn test_eval_positive_integer() {
        let mut parser = Parser::new("42");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = rocflight::Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "42");
    }

    #[test]
    fn test_eval_negative_integer() {
        let mut parser = Parser::new("-3");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = rocflight::Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "-3");
    }

    #[test]
    fn test_eval_float() {
        let mut parser = Parser::new("3.14");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = rocflight::Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "3.14");
    }

    #[test]
    fn test_eval_identifier_fails() {
        let mut parser = Parser::new("birds");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = rocflight::Evaluator::new();
        let result = eval.eval(&expr);

        // Should error - Phase 3 adds proper variable lookup
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_handling() {
        let mut parser = Parser::new("  42  ");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "42");
    }

    #[test]
    fn test_string_still_works() {
        let mut parser = Parser::new("\"hello\"");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "\"hello\"");
    }

    #[test]
    fn test_mixed_expressions_with_whitespace() {
        // Integer with whitespace
        let mut parser = Parser::new("  -99  ");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "-99");

        // Float with whitespace
        let mut parser = Parser::new("  2.71828  ");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "2.71828");

        // Identifier with whitespace
        let mut parser = Parser::new("  main  ");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert_eq!(expr.to_string(), "main");
    }
}
