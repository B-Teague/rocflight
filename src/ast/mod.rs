//! Abstract Syntax Tree definitions for Roc
//!
//! Phase 1: String literals
//! Phase 2: Numbers, identifiers
//! Phase 3: Lambdas, calls, let bindings
//! Phase 4: Builtins, lambdas, qualified names

use std::fmt;

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
        }
    }
}

/// Top-level expression
#[derive(Debug, Clone)]
pub enum Expr<'a> {
    /// String literal: "hello"
    Str(&'static str),
    /// String interpolation: "x=${expr}"
    StrInterp(Vec<StrPart<'a>>),
    /// Integer literal: 42, -3
    Int(i64),
    /// Float literal: 3.14, -2.5
    Float(f64),
    /// Identifier: x, main
    Ident(&'static str),
    /// Qualified name: Module.function
    Qualified {
        module: &'static str,
        name: &'static str,
    },
    /// Binary operation: left op right
    BinOp {
        left: Box<Expr<'a>>,
        op: BinOp,
        right: Box<Expr<'a>>,
    },
    /// Lambda function: |x| body or |x, y| x + y
    Lambda {
        params: Vec<&'static str>,
        body: Box<Expr<'a>>,
    },
    /// Function call: f(x) or add(1, 2)
    Call {
        func: Box<Expr<'a>>,
        args: Vec<Expr<'a>>,
    },
    /// Let binding: let x = value in body
    Let {
        name: &'static str,
        value: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
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
            Expr::Int(n) => write!(f, "{}", n),
            Expr::Float(n) => write!(f, "{}", n),
            Expr::Ident(name) => write!(f, "{}", name),
            Expr::Qualified { module, name } => {
                write!(f, "{}.{}", module, name)
            }
            Expr::BinOp { left, op, right } => {
                write!(f, "({} {} {})", left, op, right)
            }
            Expr::Lambda { params, body } => {
                write!(f, "|{}| {}", params.join(", "), body)
            }
            Expr::Call { func, args } => {
                write!(f, "{}(", func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Let { name, value, body } => {
                write!(f, "let {} = {} in {}", name, value, body)
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
