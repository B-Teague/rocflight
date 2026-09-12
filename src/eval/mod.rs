//! Expression evaluator (tree-walk interpreter)
//!
//! Phase 1: String evaluation
//! Phase 2: Numbers, identifiers (partial)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::Expr;
use crate::error::EvalError;

pub mod value;
pub mod environment;

pub use value::Value;
pub use environment::Environment;

/// Tree-walk interpreter
pub struct Evaluator {
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
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Ident(name) => {
                // Look up variable in environment
                match self.env.lookup(name) {
                    Some(val) => Ok(val),
                    None => Err(EvalError {
                        message: format!("Undefined variable: {}", name),
                    }),
                }
            }
            Expr::Call { func, args: _ } => {
                // Phase 3: Basic calls not yet supported
                // Phase 4 will add builtin functions and user-defined functions
                match &**func {
                    Expr::Ident(name) => {
                        Err(EvalError {
                            message: format!("Function '{}' not defined", name),
                        })
                    }
                    _ => {
                        Err(EvalError {
                            message: "Function calls not yet supported".to_string(),
                        })
                    }
                }
            }
            Expr::Let { name, value, body } => {
                // Evaluate value
                let val = self.eval(value)?;

                // Bind in environment
                self.env.bind(name, val);

                // Evaluate body
                self.eval(body)
            }
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
