//! Phase 3 Tests: Variable Binding & Function Calls
//!
//! Tests for:
//! - Let binding expressions
//! - Variable lookup in environment
//! - Function calls (basic)

#[cfg(test)]
mod phase3_tests {
    use rocflight::{Parser, TypeChecker, Evaluator};

    #[test]
    fn test_parse_let_binding() {
        let mut parser = Parser::new("let x = 42 in x");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("let x"));
    }

    #[test]
    fn test_parse_let_with_string() {
        let mut parser = Parser::new("let msg = \"hello\" in msg");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("let msg"));
    }

    #[test]
    fn test_parse_nested_let() {
        let mut parser = Parser::new("let x = 1 in let y = 2 in y");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("let"));
    }

    #[test]
    fn test_eval_let_binding_int() {
        let mut parser = Parser::new("let x = 42 in x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "42");
    }

    #[test]
    fn test_eval_let_binding_string() {
        let mut parser = Parser::new("let msg = \"hello\" in msg");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "\"hello\"");
    }

    #[test]
    fn test_eval_let_binding_unused_var() {
        let mut parser = Parser::new("let x = 42 in 99");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        // Body result is 99, not 42
        assert_eq!(val.to_string(), "99");
    }

    #[test]
    fn test_eval_nested_let() {
        let mut parser = Parser::new("let x = 1 in let y = 2 in x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        // Inner body uses x from outer scope
        assert_eq!(val.to_string(), "1");
    }

    #[test]
    fn test_eval_nested_let_shadowing() {
        let mut parser = Parser::new("let x = 1 in let x = 2 in x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        // Inner x shadows outer x, so result is 2
        // (Note: linear search finds most recent binding)
        assert_eq!(val.to_string(), "2");
    }

    #[test]
    fn test_type_check_let_binding() {
        let mut parser = Parser::new("let x = 42 in x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut checker = TypeChecker::new();
        let ty = checker.synth(&expr).expect("Failed to infer type");

        // The let's type is its body's type, and the body is `x` — which the type
        // environment now knows is an I64. This used to be an opaque `$0`, because
        // every identifier synthesised to a fresh variable.
        assert_eq!(ty.to_string(), "I64");
    }

    #[test]
    fn test_parse_function_call() {
        let mut parser = Parser::new("f(42)");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("f(42)"));
    }

    #[test]
    fn test_parse_function_call_multiple_args() {
        let mut parser = Parser::new("add(1, 2)");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("add("));
    }

    #[test]
    fn test_parse_function_call_no_args() {
        let mut parser = Parser::new("f()");
        let expr = parser.parse_expr().expect("Failed to parse");
        assert!(expr.to_string().contains("f()"));
    }

    #[test]
    fn test_eval_function_call_undefined() {
        let mut parser = Parser::new("f(42)");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let result = eval.eval(&expr);

        // Should error - function not defined
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_in_let() {
        let mut parser = Parser::new("let   x   =   42   in   x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "42");
    }

    #[test]
    fn test_let_with_complex_value() {
        let mut parser = Parser::new("let x = 3.14 in x");
        let expr = parser.parse_expr().expect("Failed to parse");

        let mut eval = Evaluator::new();
        let val = eval.eval(&expr).expect("Failed to evaluate");

        assert_eq!(val.to_string(), "3.14");
    }

    #[test]
    fn test_let_binding_in_call_args() {
        let mut parser = Parser::new("let x = 42 in f(x)");
        let expr = parser.parse_expr().expect("Failed to parse");

        // Parsing should succeed
        assert!(expr.to_string().contains("let"));
    }
}
