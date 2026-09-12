//! Error types for the Roc interpreter

use std::fmt;

/// Parse errors from the nom-based parser
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Parse error at position {}: {}", self.position, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Type checking errors
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub expected: String,
    pub actual: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Type error at {}:{}\n  Expected: {}\n  Actual: {}\n  {}",
            self.line, self.col, self.expected, self.actual, self.message
        )
    }
}

impl std::error::Error for TypeError {}

/// Runtime/evaluation errors
#[derive(Debug, Clone)]
pub struct EvalError {
    pub message: String,
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Runtime error: {}", self.message)
    }
}

impl std::error::Error for EvalError {}
