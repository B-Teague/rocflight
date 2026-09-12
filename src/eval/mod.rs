//! Expression evaluator (tree-walk interpreter)
//!
//! Phase 1: String evaluation

use crate::ast::Expr;
use crate::error::EvalError;

pub mod value;
pub mod environment;

pub use value::Value;
pub use environment::Environment;

/// Tree-walk interpreter
pub struct Evaluator {
    #[allow(dead_code)]
    env: Environment,
}

impl Evaluator {
    /// Create new evaluator
    pub fn new() -> Self {
        Evaluator {
            env: Environment::new(),
        }
    }

    /// Evaluate expression to value
    pub fn eval(&mut self, expr: &Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Str(s) => Ok(Value::Str(*s)),
            Expr::StrInterp(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        crate::ast::StrPart::Literal(s) => result.push_str(s),
                        crate::ast::StrPart::Expr(e) => {
                            // Evaluate nested expression and convert to string
                            match self.eval(e) {
                                Ok(v) => result.push_str(&v.to_string()),
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                Ok(Value::Str(Box::leak(result.into_boxed_str())))
            }
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
