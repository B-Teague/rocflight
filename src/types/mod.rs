//! Type system for Roc
//!
//! Hindley-Milner type inference with unification
//! Phase 1: Str type

use std::fmt;
use std::collections::HashMap;

pub mod checker;

pub use checker::TypeChecker;

/// Type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// String type
    Str,
    /// Integer type (64-bit signed)
    I64,
    /// Float type (64-bit)
    F64,
    /// Boolean type
    Bool,
    /// Type variable: $0, $1, etc.
    TypeVar(u32),
    /// List type: List(T)
    List(Box<Type>),
    /// Function type: (A -> B)
    Function(Box<Type>, Box<Type>),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Str => write!(f, "Str"),
            Type::I64 => write!(f, "I64"),
            Type::F64 => write!(f, "F64"),
            Type::Bool => write!(f, "Bool"),
            Type::TypeVar(n) => write!(f, "${}", n),
            Type::List(t) => write!(f, "List({})", t),
            Type::Function(a, b) => write!(f, "({} -> {})", a, b),
        }
    }
}

/// Type substitution map: maps TypeVar to concrete Type
pub struct Substitution {
    bindings: HashMap<u32, Type>,
}

impl Substitution {
    /// Create empty substitution
    pub fn new() -> Self {
        Substitution {
            bindings: HashMap::new(),
        }
    }

    /// Insert a type binding
    pub fn insert(&mut self, var: u32, ty: Type) {
        self.bindings.insert(var, ty);
    }

    /// Get a type binding
    pub fn get(&self, var: u32) -> Option<&Type> {
        self.bindings.get(&var)
    }

    /// Apply substitution to a type (follow chains)
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::TypeVar(v) => {
                if let Some(bound) = self.get(*v) {
                    self.apply(bound)
                } else {
                    ty.clone()
                }
            }
            Type::List(inner) => Type::List(Box::new(self.apply(inner))),
            Type::Function(a, b) => {
                Type::Function(Box::new(self.apply(a)), Box::new(self.apply(b)))
            }
            other => other.clone(),
        }
    }
}

impl Default for Substitution {
    fn default() -> Self {
        Self::new()
    }
}
