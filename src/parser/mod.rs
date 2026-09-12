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
    /// Handles: let bindings, function calls, literals
    pub fn parse_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.parse_let_or_expr()
    }

    /// Parse let binding or regular expression
    fn parse_let_or_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected end of input".to_string(),
                position: self.pos,
            });
        }

        // Check for "let" keyword
        if rest.starts_with("let ") {
            self.pos += 4; // Skip "let "
            self.skip_whitespace();

            // Parse variable name
            let rest = &self.input[self.pos..];
            let (remaining, name_expr) = parse_ident_nom(rest)?;
            self.pos += rest.len() - remaining.len();

            let name = match name_expr {
                Expr::Ident(n) => n,
                _ => unreachable!(),
            };

            self.skip_whitespace();

            // Expect "="
            let rest = &self.input[self.pos..];
            if !rest.starts_with('=') {
                return Err(ParseError {
                    message: "Expected '=' after variable name in let binding".to_string(),
                    position: self.pos,
                });
            }
            self.pos += 1;
            self.skip_whitespace();

            // Parse value expression
            let value = Box::new(self.parse_primary_expr()?);

            self.skip_whitespace();

            // Expect "in"
            let rest = &self.input[self.pos..];
            if !rest.starts_with("in ") {
                return Err(ParseError {
                    message: "Expected 'in' after value in let binding".to_string(),
                    position: self.pos,
                });
            }
            self.pos += 3; // Skip "in "
            self.skip_whitespace();

            // Parse body expression
            let body = Box::new(self.parse_let_or_expr()?);

            Ok(Expr::Let { name, value, body })
        } else {
            self.parse_call_expr()
        }
    }

    /// Parse function call or primary expression
    fn parse_call_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            // Check for function call
            if rest.starts_with('(') {
                self.pos += 1; // Skip '('
                self.skip_whitespace();

                let mut args = Vec::new();

                // Parse arguments
                let rest = &self.input[self.pos..];
                if !rest.starts_with(')') {
                    loop {
                        args.push(self.parse_primary_expr()?);
                        self.skip_whitespace();

                        let rest = &self.input[self.pos..];
                        if rest.starts_with(',') {
                            self.pos += 1;
                            self.skip_whitespace();
                        } else {
                            break;
                        }
                    }
                }

                self.skip_whitespace();
                let rest = &self.input[self.pos..];
                if !rest.starts_with(')') {
                    return Err(ParseError {
                        message: "Expected ')' after function arguments".to_string(),
                        position: self.pos,
                    });
                }
                self.pos += 1; // Skip ')'

                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// Parse primary expression: number, string, or identifier
    fn parse_primary_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected end of input".to_string(),
                position: self.pos,
            });
        }

        // Try number first
        if let Ok((remaining, expr)) = parse_number_nom(rest) {
            self.pos += rest.len() - remaining.len();
            self.skip_whitespace();
            return Ok(expr);
        }

        // Try string
        if rest.starts_with('"') {
            return self.parse_string();
        }

        // Try identifier
        if is_ident_start(rest.chars().next().unwrap()) {
            if let Ok((remaining, expr)) = parse_ident_nom(rest) {
                self.pos += rest.len() - remaining.len();
                self.skip_whitespace();
                return Ok(expr);
            }
        }

        // Fallback to string parsing for error message
        self.parse_string()
    }

    /// Skip whitespace
    fn skip_whitespace(&mut self) {
        let rest = &self.input[self.pos..];
        let trimmed = rest.trim_start();
        self.pos += rest.len() - trimmed.len();
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

/// Check if character can start an identifier
fn is_ident_start(c: char) -> bool {
    c.is_ascii_lowercase() || c == '_'
}

/// Check if character can be in an identifier
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Parse number literal (int or float): 42, -3, 3.14, -2.5
fn parse_number_nom(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
    let mut pos = 0;
    let input_bytes = input.as_bytes();

    // Optional minus sign
    if pos < input_bytes.len() && input_bytes[pos] == b'-' {
        pos += 1;
    }

    // Must have at least one digit
    if pos >= input_bytes.len() || !input_bytes[pos].is_ascii_digit() {
        return Err(ParseError {
            message: "Expected digit".to_string(),
            position: 0,
        });
    }

    // Parse integer part
    while pos < input_bytes.len() && input_bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    // Check for decimal point
    let is_float = pos < input_bytes.len() && input_bytes[pos] == b'.';

    if is_float {
        pos += 1; // Skip decimal point
        // Must have at least one digit after decimal
        if pos >= input_bytes.len() || !input_bytes[pos].is_ascii_digit() {
            return Err(ParseError {
                message: "Expected digit after decimal point".to_string(),
                position: pos,
            });
        }
        // Parse fractional part
        while pos < input_bytes.len() && input_bytes[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    let num_str = &input[..pos];
    let remaining = &input[pos..];

    if is_float {
        match num_str.parse::<f64>() {
            Ok(f) => Ok((remaining, Expr::Float(f))),
            Err(_) => Err(ParseError {
                message: format!("Invalid float: {}", num_str),
                position: 0,
            }),
        }
    } else {
        match num_str.parse::<i64>() {
            Ok(i) => Ok((remaining, Expr::Int(i))),
            Err(_) => Err(ParseError {
                message: format!("Invalid integer: {}", num_str),
                position: 0,
            }),
        }
    }
}

/// Parse identifier: x, main, birds
fn parse_ident_nom(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
    let mut pos = 0;
    let mut chars = input.chars();

    // First character must be lowercase letter or underscore
    match chars.next() {
        Some(c) if is_ident_start(c) => pos += c.len_utf8(),
        _ => {
            return Err(ParseError {
                message: "Expected identifier start".to_string(),
                position: 0,
            })
        }
    }

    // Rest can be alphanumeric or underscore
    for c in chars {
        if is_ident_char(c) {
            pos += c.len_utf8();
        } else {
            break;
        }
    }

    let ident = &input[..pos];
    let remaining = &input[pos..];

    Ok((remaining, Expr::Ident(string_pool::intern(ident))))
}
