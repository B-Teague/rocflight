//! Abstract Syntax Tree definitions for Roc
//!
//! Phase 1: String literals

use std::fmt;

/// Top-level expression
#[derive(Debug, Clone)]
pub enum Expr<'a> {
    /// String literal: "hello"
    Str(&'static str),
    /// String interpolation: "x=${expr}"
    StrInterp(Vec<StrPart<'a>>),
}

/// Part of a string interpolation
#[derive(Debug, Clone)]
pub enum StrPart<'a> {
    /// Literal part of string
    Literal(&'static str),
    /// Expression to interpolate: ${...}
    Expr(&'a Expr<'a>),
}

impl<'a> fmt::Display for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Str(s) => write!(f, "\"{}\"", s),
            Expr::StrInterp(parts) => {
                write!(f, "\"")?;
                for part in parts {
                    match part {
                        StrPart::Literal(s) => write!(f, "{}", s)?,
                        StrPart::Expr(e) => write!(f, "${{{}}}", e)?,
                    }
                }
                write!(f, "\"")
            }
        }
    }
}

/// Pattern for pattern matching (Phase 3+)
#[derive(Debug, Clone)]
pub enum Pattern<'a> {
    /// Wildcard: _
    Wildcard,
    /// Variable binding: x
    Var(&'static str),
    /// Literal value
    Literal(Expr<'a>),
}
