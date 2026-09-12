//! Runtime values for the evaluator

use std::fmt;

/// Runtime value
#[derive(Debug, Clone)]
pub enum Value {
    /// String value
    Str(&'static str),
    /// Integer value (64-bit signed)
    Int(i64),
    /// Float value (64-bit)
    Float(f64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "\"{}\"", s),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                // Format float without unnecessary decimals
                if n.fract() == 0.0 && n.abs() < 1e10 {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
        }
    }
}
