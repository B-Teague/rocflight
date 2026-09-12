//! Error types for the Roc interpreter

use thiserror::Error;

/// Parse errors from the nom-based parser
#[derive(Error, Debug, Clone)]
#[error("Parse error at position {position}: {message}")]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl ParseError {
    /// Create a new parse error
    pub fn new(message: impl Into<String>, position: usize) -> Self {
        ParseError {
            message: message.into(),
            position,
        }
    }

    /// Create a parse error with line/column information computed from input
    pub fn with_location(message: impl Into<String>, position: usize, _input: &str) -> Self {
        // Note: Line/column computation available via compute_location()
        // Future versions will integrate this into the error display
        ParseError {
            message: message.into(),
            position,
        }
    }

    /// Compute line and column from position in input
    pub fn compute_location(position: usize, input: &str) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;
        for (i, ch) in input.chars().enumerate() {
            if i >= position {
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }
}

/// Type checking errors
#[derive(Error, Debug, Clone)]
#[error("Type error at {line}:{col}\n  Expected: {expected}\n  Actual: {actual}\n  {message}")]
pub struct TypeError {
    pub message: String,
    pub expected: String,
    pub actual: String,
    pub line: usize,
    pub col: usize,
}

impl TypeError {
    /// Create a new type error
    pub fn new(
        message: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
        line: usize,
        col: usize,
    ) -> Self {
        TypeError {
            message: message.into(),
            expected: expected.into(),
            actual: actual.into(),
            line,
            col,
        }
    }
}

/// Runtime/evaluation errors
#[derive(Error, Debug, Clone)]
#[error("Runtime error: {message}")]
pub struct EvalError {
    pub message: String,
}

impl EvalError {
    /// Create a new evaluation error
    pub fn new(message: impl Into<String>) -> Self {
        EvalError {
            message: message.into(),
        }
    }
}
