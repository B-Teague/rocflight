//! Type checking and inference
//!
//! Bidirectional type checking: synthesis (infer) + checking (verify)

use crate::ast::Expr;
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
                // Phase 3 will add environment lookup
                Ok(self.fresh_var())
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
