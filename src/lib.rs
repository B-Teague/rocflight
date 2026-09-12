//! Roc Language Interpreter
//!
//! A tree-walk interpreter for the Roc programming language, built in Rust.
//! Features full type checking with Hindley-Milner inference and memory optimizations.
//!
//! Pipeline:
//! 1. Load .roc file
//! 2. Desugar shorthand syntax (!, ?, ??, .?, ?:)
//! 3. Parse into AST
//! 4. Type check with Hindley-Milner inference
//! 5. Evaluate with tree-walk interpreter

pub mod ast;
pub mod types;
pub mod parser;
pub mod eval;
pub mod memory;
pub mod error;
pub mod desugaring;
pub mod platform;

pub use ast::{Expr, Pattern};
pub use types::Type;
pub use eval::Evaluator;
pub use error::{ParseError, TypeError};
pub use desugaring::Desugarer;
pub use platform::{PlatformLoader, Platform, PlatformRef};
