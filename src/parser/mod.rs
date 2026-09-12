//! Parser for Roc using nom
//!
//! Pipeline:
//! 1. Desugar shorthand syntax
//! 2. Parse desugared code
//! 3. Build AST

use crate::ast::{Expr, StrPart};
use crate::error::ParseError;
use crate::memory::string_pool;
use crate::desugaring::Desugarer;

/// Roc parser
pub struct Parser {
    input: String,
    pos: usize,
}

impl Parser {
    /// Create new parser for input
    pub fn new(input: &str) -> Self {
        Parser {
            input: input.to_string(),
            pos: 0,
        }
    }

    /// Load and parse file (with desugaring)
    pub fn from_file(path: &str) -> Result<Expr<'static>, ParseError> {
        // Step 1: Load file
        let source = std::fs::read_to_string(path)
            .map_err(|e| ParseError {
                message: format!("Failed to read file: {}", e),
                position: 0,
            })?;

        // Step 2: Desugar shorthand syntax
        let desugarer = Desugarer::new(source.clone());
        let desugared = desugarer.desugar()?;

        // Step 3: Save desugared for debugging
        let _ = desugarer.save_debug(path, &desugared);

        // Step 4: Parse desugared code
        let mut parser = Parser::new(&desugared);
        parser.parse_expr()
    }

    /// Parse expression (entry point)
    pub fn parse_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.parse_string()
    }

    /// Parse string literal: "..."
    fn parse_string(&mut self) -> Result<Expr<'static>, ParseError> {
        let rest = &self.input[self.pos..];
        match parse_string_nom(rest) {
            Ok((remaining, expr)) => {
                self.pos += rest.len() - remaining.len();
                Ok(expr)
            }
            Err(e) => Err(ParseError {
                message: e.message,
                position: self.pos + e.position,
            }),
        }
    }
}

/// Parse string literal with interpolation
fn parse_string_nom(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
    // Check for opening quote
    if !input.starts_with('"') {
        return Err(ParseError {
            message: "Expected '\"'".to_string(),
            position: 0,
        });
    }

    let input = &input[1..]; // Skip opening quote
    let (content, remaining) = parse_string_content(input)?;

    if !remaining.starts_with('"') {
        return Err(ParseError {
            message: "Expected closing '\"'".to_string(),
            position: input.len() - remaining.len(),
        });
    }

    let remaining = &remaining[1..]; // Skip closing quote

    if content.is_empty() {
        Ok((remaining, Expr::Str(string_pool::intern(""))))
    } else if content.contains("${") {
        // Has interpolation - for Phase 1, just store as literal
        let mut parts = Vec::new();
        parts.push(StrPart::Literal(string_pool::intern(&content)));
        Ok((remaining, Expr::StrInterp(parts)))
    } else {
        // Plain string
        Ok((remaining, Expr::Str(string_pool::intern(&content))))
    }
}

/// Parse string content (everything between quotes, handling escapes)
fn parse_string_content(input: &str) -> Result<(String, &str), ParseError> {
    let mut result = String::new();
    let mut pos = 0;
    let input_bytes = input.as_bytes();

    while pos < input_bytes.len() {
        match input_bytes[pos] {
            b'"' => break,
            b'\\' => {
                pos += 1;
                if pos < input_bytes.len() {
                    match input_bytes[pos] {
                        b'n' => result.push('\n'),
                        b't' => result.push('\t'),
                        b'r' => result.push('\r'),
                        b'\\' => result.push('\\'),
                        b'"' => result.push('"'),
                        c => {
                            result.push('\\');
                            result.push(c as char);
                        }
                    }
                    pos += 1;
                }
            }
            c => {
                result.push(c as char);
                pos += 1;
            }
        }
    }

    Ok((result, &input[pos..]))
}
