//! Expression evaluator (tree-walk interpreter)
//!
//! Phase 1: String evaluation
//! Phase 2: Numbers, identifiers (partial)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::{Expr, BinOp};
use crate::error::EvalError;

pub mod value;
pub mod environment;

pub use value::Value;
pub use environment::Environment;

/// Tree-walk interpreter
pub struct Evaluator {
    pub env: Environment,
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
                                        Value::Lambda { params, .. } => format!("<lambda |{}|>", params.join(", ")),
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
            Expr::BinOp { left, op, right } => {
                let left_val = self.eval(left)?;
                let right_val = self.eval(right)?;
                self.apply_binop(*op, left_val, right_val)
            }
            Expr::Lambda { params, body } => {
                // Create a closure capturing the current environment
                // SAFETY: We transmute to 'static because the parsed AST remains
                // valid for the lifetime of the program. The body is part of the
                // parsed source, which we keep in memory.
                let static_body = unsafe {
                    std::mem::transmute::<Box<Expr<'_>>, Box<Expr<'static>>>(
                        Box::new(body.as_ref().clone())
                    )
                };
                Ok(Value::Lambda {
                    params: params.clone(),
                    body: static_body,
                    env: self.env.clone(),
                })
            }
            Expr::Call { func, args } => {
                // Handle builtin functions and calls
                match &**func {
                    Expr::Qualified { module, name } => {
                        self.call_builtin(module, name, args)
                    }
                    Expr::Ident(name) => {
                        // First check if it's a variable bound to a lambda
                        if let Some(val) = self.env.lookup(name) {
                            match val {
                                Value::Lambda { params, body, env: lambda_env } => {
                                    // Call the lambda
                                    if args.len() != params.len() {
                                        return Err(EvalError {
                                            message: format!("Lambda expects {} arguments, got {}", params.len(), args.len()),
                                        });
                                    }

                                    // Evaluate arguments in current environment
                                    let mut arg_vals = Vec::new();
                                    for arg in args {
                                        arg_vals.push(self.eval(arg)?);
                                    }

                                    // Create new evaluator with lambda's captured environment
                                    let mut lambda_eval = Evaluator { env: lambda_env };
                                    lambda_eval.env.push_scope();

                                    // Bind parameters to arguments
                                    for (param, arg_val) in params.iter().zip(arg_vals.iter()) {
                                        lambda_eval.env.bind(param, arg_val.clone());
                                    }

                                    // Evaluate body in lambda's environment
                                    return lambda_eval.eval(&body);
                                }
                                _ => {}  // Not a lambda, fall through
                            }
                        }

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
                        // Evaluate the function expression (e.g., for chained calls like f()(x))
                        let func_val = self.eval(func)?;
                        match func_val {
                            Value::Lambda { params, body, env: lambda_env } => {
                                // Call the lambda
                                if args.len() != params.len() {
                                    return Err(EvalError {
                                        message: format!("Lambda expects {} arguments, got {}", params.len(), args.len()),
                                    });
                                }

                                // Evaluate arguments in current environment
                                let mut arg_vals = Vec::new();
                                for arg in args {
                                    arg_vals.push(self.eval(arg)?);
                                }

                                // Create new evaluator with lambda's captured environment
                                let mut lambda_eval = Evaluator { env: lambda_env };
                                lambda_eval.env.push_scope();

                                // Bind parameters to arguments
                                for (param, arg_val) in params.iter().zip(arg_vals.iter()) {
                                    lambda_eval.env.bind(param, arg_val.clone());
                                }

                                // Evaluate body in lambda's environment
                                lambda_eval.eval(&body)
                            }
                            _ => {
                                Err(EvalError {
                                    message: "Attempted to call a non-function value".to_string(),
                                })
                            }
                        }
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

    /// Apply binary operation
    fn apply_binop(&self, op: BinOp, left: Value, right: Value) -> Result<Value, EvalError> {
        match (op, left, right) {
            // Arithmetic on integers
            (BinOp::Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (BinOp::Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (BinOp::Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (BinOp::Div, Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Int(a / b))
                }
            }
            // Arithmetic on floats
            (BinOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (BinOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (BinOp::Div, Value::Float(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            // Mixed int/float arithmetic
            (BinOp::Add, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
            (BinOp::Add, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
            (BinOp::Sub, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
            (BinOp::Sub, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - b as f64)),
            (BinOp::Mul, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
            (BinOp::Mul, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
            (BinOp::Div, Value::Int(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a as f64 / b))
                }
            }
            (BinOp::Div, Value::Float(a), Value::Int(b)) => {
                if b == 0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a / b as f64))
                }
            }
            // String concatenation
            (BinOp::Add, Value::Str(a), Value::Str(b)) => {
                let concatenated = format!("{}{}", a, b);
                Ok(Value::Str(std::boxed::Box::leak(concatenated.into_boxed_str())))
            }
            // Comparison operators
            (BinOp::Eq, a, b) => Ok(Value::Int(if values_equal(&a, &b) { 1 } else { 0 })),
            (BinOp::Ne, a, b) => Ok(Value::Int(if !values_equal(&a, &b) { 1 } else { 0 })),
            (BinOp::Lt, Value::Int(a), Value::Int(b)) => Ok(Value::Int(if a < b { 1 } else { 0 })),
            (BinOp::Lt, Value::Float(a), Value::Float(b)) => Ok(Value::Int(if a < b { 1 } else { 0 })),
            (BinOp::Lt, Value::Int(a), Value::Float(b)) => Ok(Value::Int(if (a as f64) < b { 1 } else { 0 })),
            (BinOp::Lt, Value::Float(a), Value::Int(b)) => Ok(Value::Int(if a < (b as f64) { 1 } else { 0 })),
            (BinOp::Le, Value::Int(a), Value::Int(b)) => Ok(Value::Int(if a <= b { 1 } else { 0 })),
            (BinOp::Le, Value::Float(a), Value::Float(b)) => Ok(Value::Int(if a <= b { 1 } else { 0 })),
            (BinOp::Le, Value::Int(a), Value::Float(b)) => Ok(Value::Int(if (a as f64) <= b { 1 } else { 0 })),
            (BinOp::Le, Value::Float(a), Value::Int(b)) => Ok(Value::Int(if a <= (b as f64) { 1 } else { 0 })),
            (BinOp::Gt, Value::Int(a), Value::Int(b)) => Ok(Value::Int(if a > b { 1 } else { 0 })),
            (BinOp::Gt, Value::Float(a), Value::Float(b)) => Ok(Value::Int(if a > b { 1 } else { 0 })),
            (BinOp::Gt, Value::Int(a), Value::Float(b)) => Ok(Value::Int(if (a as f64) > b { 1 } else { 0 })),
            (BinOp::Gt, Value::Float(a), Value::Int(b)) => Ok(Value::Int(if a > (b as f64) { 1 } else { 0 })),
            (BinOp::Ge, Value::Int(a), Value::Int(b)) => Ok(Value::Int(if a >= b { 1 } else { 0 })),
            (BinOp::Ge, Value::Float(a), Value::Float(b)) => Ok(Value::Int(if a >= b { 1 } else { 0 })),
            (BinOp::Ge, Value::Int(a), Value::Float(b)) => Ok(Value::Int(if (a as f64) >= b { 1 } else { 0 })),
            (BinOp::Ge, Value::Float(a), Value::Int(b)) => Ok(Value::Int(if a >= (b as f64) { 1 } else { 0 })),
            // Logical operators (non-zero = true, zero = false)
            (BinOp::And, a, b) => {
                let a_truthy = is_truthy(&a);
                let b_truthy = is_truthy(&b);
                Ok(Value::Int(if a_truthy && b_truthy { 1 } else { 0 }))
            }
            (BinOp::Or, a, b) => {
                let a_truthy = is_truthy(&a);
                let b_truthy = is_truthy(&b);
                Ok(Value::Int(if a_truthy || b_truthy { 1 } else { 0 }))
            }
            (_op, _left, _right) => {
                Err(EvalError {
                    message: "Invalid operands for operator".to_string(),
                })
            }
        }
    }
}

/// Check if two values are equal
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(s1), Value::Str(s2)) => s1 == s2,
        (Value::Int(n1), Value::Int(n2)) => n1 == n2,
        (Value::Float(f1), Value::Float(f2)) => (f1 - f2).abs() < 1e-10,
        (Value::Int(n), Value::Float(f)) => ((*n as f64) - f).abs() < 1e-10,
        (Value::Float(f), Value::Int(n)) => (f - (*n as f64)).abs() < 1e-10,
        _ => false,
    }
}

/// Check if a value is truthy (non-zero for numbers, non-empty for strings)
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        _ => true, // lambdas and builtins are truthy
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
