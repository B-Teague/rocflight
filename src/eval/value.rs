//! Runtime values for the evaluator

use std::fmt;

/// Runtime value
#[derive(Debug, Clone)]
pub enum Value {
    /// String value
    Str(&'static str),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "\"{}\"", s),
        }
    }
}
