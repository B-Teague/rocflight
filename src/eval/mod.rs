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
                                Ok(v) => {
                                    // Convert value to string without quotes
                                    let s = match &v {
                                        Value::Str(s) => s.to_string(),  // No extra quotes for strings
                                        Value::Int(n) => n.to_string(),
                                        Value::Float(f) => {
                                            // Format float properly
                                            if f.fract() == 0.0 && f.abs() < 1e10 {
                                                format!("{:.1}", f)
                                            } else {
                                                f.to_string()
                                            }
                                        }
                                        Value::Builtin(name, arity) => format!("<{}/{}>", name, arity),
                                    };
                                    result.push_str(&s);
                                }
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
            Expr::Qualified { module, name } => {
                // Handle builtin functions from modules
                match (*module, *name) {
                    ("Num", "to_str") => {
                        // Return a builtin function marker
                        // We'll handle it in Call evaluation
                        Err(EvalError {
                            message: "Num.to_str requires arguments".to_string(),
                        })
                    }
                    ("Stdout", "line") => {
                        Err(EvalError {
                            message: "Stdout.line requires arguments".to_string(),
                        })
                    }
                    _ => Err(EvalError {
                        message: format!("Unknown function {}.{}", module, name),
                    }),
                }
            }
            Expr::Lambda { params, .. } => {
                // Return a builtin marker for lambdas
                // Full lambda support (with closure capture) comes in Phase 5
                Ok(Value::Builtin(
                    format!("lambda/{}", params.len()),
                    params.len(),
                ))
            }
            Expr::Call { func, args } => {
                // Handle builtin functions and calls
                match &**func {
                    Expr::Qualified { module, name } => {
                        self.call_builtin(module, name, args)
                    }
                    Expr::Ident(name) => {
                        // Try to call as builtin without module
                        match *name {
                            "to_str" => {
                                // Num.to_str(value)
                                if args.len() != 1 {
                                    return Err(EvalError {
                                        message: format!("to_str expects 1 argument, got {}", args.len()),
                                    });
                                }
                                let val = self.eval(&args[0])?;
                                Ok(Value::Str(std::boxed::Box::leak(val.to_string().into_boxed_str())))
                            }
                            _ => {
                                Err(EvalError {
                                    message: format!("Function '{}' not defined", name),
                                })
                            }
                        }
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

    /// Call builtin function from a module
    fn call_builtin(&mut self, module: &str, name: &str, args: &[Expr]) -> Result<Value, EvalError> {
        match (module, name) {
            ("Num", "to_str") => {
                if args.len() != 1 {
                    return Err(EvalError {
                        message: format!("Num.to_str expects 1 argument, got {}", args.len()),
                    });
                }
                let val = self.eval(&args[0])?;
                Ok(Value::Str(std::boxed::Box::leak(val.to_string().into_boxed_str())))
            }
            ("Stdout", "line") => {
                if args.len() != 1 {
                    return Err(EvalError {
                        message: format!("Stdout.line expects 1 argument, got {}", args.len()),
                    });
                }
                let val = self.eval(&args[0])?;
                // Print to stdout, handling string formatting
                let output = match &val {
                    Value::Str(s) => s.to_string(),
                    _ => val.to_string(),
                };
                println!("{}", output);
                // Return empty value
                Ok(Value::Str(std::boxed::Box::leak(String::new().into_boxed_str())))
            }
            ("Str", "concat") => {
                let mut result = String::new();
                for arg in args {
                    let val = self.eval(arg)?;
                    match val {
                        Value::Str(s) => result.push_str(s),
                        _ => result.push_str(&val.to_string()),
                    }
                }
                Ok(Value::Str(std::boxed::Box::leak(result.into_boxed_str())))
            }
            _ => Err(EvalError {
                message: format!("Unknown function {}.{}", module, name),
            }),
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
