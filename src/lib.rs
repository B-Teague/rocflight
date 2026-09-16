// No `unsafe`, anywhere, enforced by the compiler. An interpreter's whole job is
// handing untrusted structure to a runtime, and a wrong opcode or a stale index should
// be a panic with a message — not a silent memory error that surfaces as a wrong
// answer in a golden pair three phases later. The one `unsafe` this crate used to
// contain was a lifetime transmute around the AST; dropping `Expr`'s vestigial
// lifetime parameter removed the need for it.
#![forbid(unsafe_code)]

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
pub mod vm;
pub mod memory;
pub mod error;
pub mod desugaring;
pub mod platform;

pub use ast::{Expr, Pattern};
pub use types::Type;
pub use types::TypeChecker;
pub use parser::Parser;
pub use eval::Evaluator;
pub use error::{ParseError, TypeError};
pub use desugaring::Desugarer;
pub use platform::{PlatformLoader, Platform, PlatformRef};
