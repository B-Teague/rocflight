//! Type checking and inference
//!
//! Bidirectional type checking: synthesis (infer) + checking (verify)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::{Expr, BinOp};
use crate::error::TypeError;
use super::{Type, Substitution};

/// Type checker with Hindley-Milner inference
pub struct TypeChecker {
    subst: Substitution,
    next_var: u32,
}

impl TypeChecker {
    /// Create new type checker
    pub fn new() -> Self {
        TypeChecker {
            subst: Substitution::new(),
            next_var: 0,
        }
    }

    /// Generate a fresh type variable
    pub fn fresh_var(&mut self) -> Type {
        let var = Type::TypeVar(self.next_var);
        self.next_var += 1;
        var
    }

    /// Synthesize (infer) type of expression
    pub fn synth(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Str(_) => Ok(Type::Str),
            Expr::StrInterp(_) => Ok(Type::Str), // Interpolation always produces Str
            Expr::Int(_) => Ok(Type::I64),       // Integer literals default to I64
            Expr::Float(_) => Ok(Type::F64),     // Float literals default to F64
            Expr::Ident(_) => {
                // For now, return a fresh type var for identifiers
                // Phase 4 will add environment lookup with actual types
                Ok(self.fresh_var())
            }
            Expr::Qualified { module: _, name: _ } => {
                // Qualified names are typically functions from builtins
                // Create a function type that can accept arguments
                // Input type: fresh var, Output type: fresh var
                let input_type = self.fresh_var();
                let output_type = self.fresh_var();
                Ok(Type::Function(
                    Box::new(input_type),
                    Box::new(output_type),
                ))
            }
            Expr::BinOp { left, op, right } => {
                let left_type = self.synth(left)?;
                let right_type = self.synth(right)?;

                match op {
                    // Arithmetic operators return same numeric type
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        // For now, allow any combination and return I64 as result
                        // More sophisticated type system would track numeric precision
                        self.unify(&left_type, &right_type)?;
                        Ok(Type::I64)
                    }
                    // Comparison operators always return I64 (0 or 1)
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        Ok(Type::I64)
                    }
                    // Logical operators return I64
                    BinOp::And | BinOp::Or => {
                        Ok(Type::I64)
                    }
                }
            }
            Expr::Lambda { params, body } => {
                // For each parameter, allocate a fresh type variable
                let mut param_types = vec![];
                for _ in params {
                    param_types.push(self.fresh_var());
                }

                // Infer body type
                let body_type = self.synth(body)?;

                // Build function type: (T1 -> T2 -> ... -> Tn)
                let mut result_type = body_type;
                for param_type in param_types.into_iter().rev() {
                    result_type = Type::Function(Box::new(param_type), Box::new(result_type));
                }

                Ok(result_type)
            }
            Expr::Call { func, args } => {
                // Function type must be a function
                let func_type = self.synth(func)?;

                // For each argument, check it unifies with parameter type
                let mut current_type = func_type;
                for arg in args {
                    match current_type {
                        Type::Function(param_type, return_type) => {
                            let arg_type = self.synth(arg)?;
                            self.unify(&arg_type, &param_type)?;
                            current_type = *return_type;
                        }
                        _ => {
                            return Err(TypeError {
                                message: format!("Cannot call non-function type: {}", current_type),
                                expected: "function type".to_string(),
                                actual: current_type.to_string(),
                                line: 0,
                                col: 0,
                            })
                        }
                    }
                }

                Ok(current_type)
            }
            Expr::Let { value, body, .. } => {
                // Type of let is the type of the body
                // The value type must be compatible with how it's used in body
                let _ = self.synth(value)?;
                self.synth(body)
            }
        }
    }

    /// Check expression against expected type
    pub fn check(&mut self, expr: &Expr, expected: &Type) -> Result<(), TypeError> {
        let inferred = self.synth(expr)?;
        self.unify(&inferred, expected)
    }

    /// Unify two types
    pub fn unify(&mut self, t1: &Type, t2: &Type) -> Result<(), TypeError> {
        let t1 = self.subst.apply(t1);
        let t2 = self.subst.apply(t2);

        if t1 == t2 {
            return Ok(());
        }

        match (&t1, &t2) {
            // TypeVar cases
            (Type::TypeVar(v1), Type::TypeVar(v2)) if v1 == v2 => Ok(()),
            (Type::TypeVar(v), t) | (t, Type::TypeVar(v)) => {
                if self.occurs_check(*v, t) {
                    Err(TypeError {
                        message: format!("Infinite type: ${} = {}", v, t),
                        expected: t1.to_string(),
                        actual: t2.to_string(),
                        line: 0,
                        col: 0,
                    })
                } else {
                    self.subst.insert(*v, t.clone());
                    Ok(())
                }
            }
            // List unification
            (Type::List(a), Type::List(b)) => self.unify(a, b),
            // Function unification
            (Type::Function(a1, b1), Type::Function(a2, b2)) => {
                self.unify(a1, a2)?;
                self.unify(b1, b2)
            }
            // Type mismatch
            _ => Err(TypeError {
                message: format!("Cannot unify {} with {}", t1, t2),
                expected: t1.to_string(),
                actual: t2.to_string(),
                line: 0,
                col: 0,
            }),
        }
    }

    /// Occurs check: prevent infinite types
    fn occurs_check(&self, var: u32, ty: &Type) -> bool {
        let ty = self.subst.apply(ty);
        match ty {
            Type::TypeVar(v) => v == var,
            Type::List(inner) => self.occurs_check(var, &inner),
            Type::Function(a, b) => self.occurs_check(var, &a) || self.occurs_check(var, &b),
            _ => false,
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
