//! Pure Functional Parser for Roc
//!
//! Built with pure functional combinators (no external parser libraries)
//! Follows idiomatic Rust with Result-based error handling
//!
//! Pipeline:
//! 1. Desugar shorthand syntax
//! 2. Parse desugared code
//! 3. Build AST
//!
//! Parser Architecture:
//! - Recursive descent with explicit precedence levels
//! - Pure functions that return Result<(T, &str), ParseError>
//! - No side effects or external dependencies
//! - Full transparency and control over parsing behavior

use crate::ast::{Expr, StrPart};
use crate::error::ParseError;
use crate::memory::string_pool;
use crate::desugaring::Desugarer;

/// Roc parser
pub struct Parser {
    input: String,
    pos: usize,
    /// App entry point (e.g., "main!") if app declaration found
    entry_point: Option<String>,
}

impl Parser {
    /// Create new parser for input
    pub fn new(input: &str) -> Self {
        Parser {
            input: input.to_string(),
            pos: 0,
            entry_point: None,
        }
    }

    /// Load and parse file (with desugaring)
    /// Returns: (AST, app_entry_point)
    pub fn from_file(path: &str) -> Result<(Expr<'static>, Option<String>), ParseError> {
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
        let expr = parser.parse_expr()?;
        Ok((expr, parser.app_entry_point()))
    }

    /// Get the app entry point if one was found
    pub fn app_entry_point(&self) -> Option<String> {
        self.entry_point.clone()
    }

    /// Parse expression (entry point)
    /// Handles: let bindings, function calls, literals, top-level definitions
    pub fn parse_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.skip_whitespace();

        // Skip app and import declarations at the top level
        loop {
            let rest = &self.input[self.pos..];

            if rest.starts_with("app ") {
                // Extract entry point from app declaration: app [entry!] { ... }
                self.extract_app_entry_point();
                self.skip_to_next_declaration();
                self.skip_whitespace();
            } else if rest.starts_with("import ") {
                self.skip_to_line_end();
                self.skip_whitespace();
            } else {
                break;
            }
        }

        // Parse the main expression
        self.parse_let_or_expr()
    }

    /// Extract app entry point from declaration like: app [main!] { ... }
    fn extract_app_entry_point(&mut self) {
        self.pos += 4; // Skip "app "
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.starts_with('[') {
            self.pos += 1; // Skip '['
            self.skip_whitespace();

            let rest = &self.input[self.pos..];
            // Find the identifier (entry point), including trailing '!'
            let mut end = 0;
            for (i, ch) in rest.chars().enumerate() {
                if ch == ']' || ch.is_whitespace() {
                    end = i;
                    break;
                }
                if i == rest.len() - 1 {
                    end = rest.len();
                }
            }

            if end > 0 {
                let entry_point = rest[..end].to_string();
                self.entry_point = Some(entry_point);
                self.pos += end;
            }
        }
    }

    /// Skip until next declaration or expression
    fn skip_to_next_declaration(&mut self) {
        while self.pos < self.input.len() {
            let rest = &self.input[self.pos..];
            if rest.starts_with('\n') {
                self.pos += 1;
                self.skip_whitespace();
                return;
            }
            self.pos += 1;
        }
    }

    /// Skip to end of line
    fn skip_to_line_end(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'\n' {
            self.pos += 1;
        }
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
            let (remaining, name_expr) = parse_identifier(rest)?;
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
            if !rest.starts_with("in") {
                return Err(ParseError {
                    message: "Expected 'in' after value in let binding".to_string(),
                    position: self.pos,
                });
            }
            // Make sure "in" is followed by whitespace or end of input
            let after_in = &rest[2..];
            if !after_in.is_empty() && !after_in.starts_with(|c: char| c.is_whitespace()) {
                return Err(ParseError {
                    message: "Expected whitespace or end of input after 'in'".to_string(),
                    position: self.pos + 2,
                });
            }
            self.pos += 2; // Skip "in"
            self.skip_whitespace();

            // Parse body expression
            let body = Box::new(self.parse_let_or_expr()?);

            Ok(Expr::Let { name, value, body })
        } else {
            // Check for top-level binding: name = expr
            // This is similar to let but at file level
            let lookahead_rest = rest;
            if let Ok((remaining, expr)) = parse_identifier(lookahead_rest) {
                let lookahead_pos = lookahead_rest.len() - remaining.len();
                let after_ident = &lookahead_rest[lookahead_pos..].trim_start();

                if after_ident.starts_with('=') && !after_ident.starts_with("==") {
                    // This is a binding!
                    if let Expr::Ident(name) = expr {
                        self.pos += lookahead_pos;
                        self.skip_whitespace();
                        self.pos += 1; // Skip '='
                        self.skip_whitespace();

                        // Parse value
                        let value = Box::new(self.parse_call_expr()?);

                        self.skip_whitespace();

                        // Check if there's more content
                        let rest3 = &self.input[self.pos..];
                        if rest3.is_empty() {
                            // If nothing after, create a let binding with the value as body
                            // This will return the value
                            return Ok(Expr::Let {
                                name,
                                value: value.clone(),
                                body: value,
                            });
                        } else {
                            // Continue parsing
                            self.skip_whitespace();
                            let body = Box::new(self.parse_let_or_expr()?);
                            return Ok(Expr::Let { name, value, body });
                        }
                    }
                }
            }

            self.parse_or_expr()
        }
    }

    /// Parse logical OR: a || b
    fn parse_or_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut left = self.parse_and_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            if rest.starts_with("||") && !rest.starts_with("|||") {
                self.pos += 2;
                self.skip_whitespace();
                let right = self.parse_and_expr()?;
                left = Expr::BinOp {
                    left: Box::new(left),
                    op: crate::ast::BinOp::Or,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    /// Parse logical AND: a && b
    fn parse_and_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut left = self.parse_comparison_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            if rest.starts_with("&&") && !rest.starts_with("&&&") {
                self.pos += 2;
                self.skip_whitespace();
                let right = self.parse_comparison_expr()?;
                left = Expr::BinOp {
                    left: Box::new(left),
                    op: crate::ast::BinOp::And,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    /// Parse comparison: a == b, a < b, etc.
    fn parse_comparison_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut left = self.parse_additive_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let op = if rest.starts_with("==") {
                self.pos += 2;
                crate::ast::BinOp::Eq
            } else if rest.starts_with("!=") {
                self.pos += 2;
                crate::ast::BinOp::Ne
            } else if rest.starts_with("<=") {
                self.pos += 2;
                crate::ast::BinOp::Le
            } else if rest.starts_with(">=") {
                self.pos += 2;
                crate::ast::BinOp::Ge
            } else if rest.starts_with('<') && !rest.starts_with("<<") {
                self.pos += 1;
                crate::ast::BinOp::Lt
            } else if rest.starts_with('>') && !rest.starts_with(">>") {
                self.pos += 1;
                crate::ast::BinOp::Gt
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_additive_expr()?;
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse addition/subtraction: a + b, a - b
    fn parse_additive_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut left = self.parse_multiplicative_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let op = if rest.starts_with('+') {
                self.pos += 1;
                crate::ast::BinOp::Add
            } else if rest.starts_with('-') && !is_next_digit(rest) {
                self.pos += 1;
                crate::ast::BinOp::Sub
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_multiplicative_expr()?;
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse multiplication/division: a * b, a / b
    fn parse_multiplicative_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        let mut left = self.parse_call_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let op = if rest.starts_with('*') {
                self.pos += 1;
                crate::ast::BinOp::Mul
            } else if rest.starts_with('/') {
                self.pos += 1;
                crate::ast::BinOp::Div
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_call_expr()?;
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
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

    /// Parse primary expression: number, string, identifier, lambda
    fn parse_primary_expr(&mut self) -> Result<Expr<'static>, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected end of input".to_string(),
                position: self.pos,
            });
        }

        // Try lambda first: |x| body or |x, y| body
        if rest.starts_with('|') {
            return self.parse_lambda();
        }

        // Try number
        if let Ok((remaining, expr)) = parse_number_literal(rest) {
            self.pos += rest.len() - remaining.len();
            self.skip_whitespace();
            return Ok(expr);
        }

        // Try string
        if rest.starts_with('"') {
            return self.parse_string();
        }

        // Try identifier or qualified name
        if let Some(first_char) = rest.chars().next() {
            if is_ident_start(first_char) {
                if let Ok((remaining, expr)) = parse_identifier(rest) {
                    self.pos += rest.len() - remaining.len();

                    // Check for qualified name: Module.function
                    let rest2 = &self.input[self.pos..];
                    if rest2.starts_with('.') {
                        let after_dot = &rest2[1..];
                        if let Some(after_dot_char) = after_dot.chars().next() {
                                if is_ident_start(after_dot_char) {
                                    self.pos += 1; // Skip '.'
                                    if let Expr::Ident(module) = expr {
                                        if let Ok((remaining, name_expr)) = parse_identifier(&self.input[self.pos..]) {
                                            self.pos += self.input[self.pos..].len() - remaining.len();
                                            if let Expr::Ident(name) = name_expr {
                                                self.skip_whitespace();
                                                return Ok(Expr::Qualified { module, name });
                                            }
                                        }
                                    }
                                }
                            }
                        }

                    self.skip_whitespace();
                    return Ok(expr);
                }
            }
        }

        // Fallback to string parsing for error message
        self.parse_string()
    }

    /// Parse lambda expression: |params| body
    fn parse_lambda(&mut self) -> Result<Expr<'static>, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if !rest.starts_with('|') {
            return Err(ParseError {
                message: "Expected '|' to start lambda".to_string(),
                position: self.pos,
            });
        }
        self.pos += 1; // Skip '|'
        self.skip_whitespace();

        let mut params = Vec::new();

        // Parse parameters
        let rest = &self.input[self.pos..];
        if !rest.starts_with('|') {
            loop {
                let rest = &self.input[self.pos..];
                if let Ok((remaining, param_expr)) = parse_identifier(rest) {
                    self.pos += rest.len() - remaining.len();
                    if let Expr::Ident(param) = param_expr {
                        params.push(param);
                    }

                    self.skip_whitespace();
                    let rest = &self.input[self.pos..];

                    if rest.starts_with(',') {
                        self.pos += 1;
                        self.skip_whitespace();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.skip_whitespace();
        let rest = &self.input[self.pos..];
        if !rest.starts_with('|') {
            return Err(ParseError {
                message: "Expected '|' to end lambda parameters".to_string(),
                position: self.pos,
            });
        }
        self.pos += 1; // Skip '|'
        self.skip_whitespace();

        // Parse body
        let body = Box::new(self.parse_expr()?);

        Ok(Expr::Lambda { params, body })
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
        match parse_string_literal(rest) {
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
fn parse_string_literal(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
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
        // Parse interpolation expressions
        let parts = parse_interpolation_parts(&content)?;
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
/// Allows both lowercase and uppercase for module names
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// Check if character can be in an identifier
/// Allows both lowercase and uppercase
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Parse number literal: decimal, hex (0xFF), octal (0o77), binary (0b1010)
/// With optional type suffixes: .U8, .I32, .F32, .Dec
fn parse_number_literal(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
    let mut pos = 0;
    let input_bytes = input.as_bytes();

    // Optional minus sign
    let is_negative = if pos < input_bytes.len() && input_bytes[pos] == b'-' {
        pos += 1;
        true
    } else {
        false
    };

    // Must have at least one digit or start of special format
    if pos >= input_bytes.len() || !input_bytes[pos].is_ascii_digit() {
        return Err(ParseError {
            message: "Expected digit".to_string(),
            position: 0,
        });
    }

    // Check for hex (0x), octal (0o), or binary (0b) formats
    if input_bytes[pos] == b'0' && pos + 1 < input_bytes.len() {
        match input_bytes[pos + 1] {
            b'x' | b'X' => {
                // Hex format: 0xFF
                pos += 2; // Skip "0x"
                let hex_start = pos;
                while pos < input_bytes.len() && input_bytes[pos].is_ascii_hexdigit() {
                    pos += 1;
                }
                if pos == hex_start {
                    return Err(ParseError {
                        message: "Expected hex digit after 0x".to_string(),
                        position: pos,
                    });
                }
                let hex_str = &input[hex_start..pos];
                let num_val = i64::from_str_radix(hex_str, 16)
                    .map_err(|_| ParseError {
                        message: format!("Invalid hex number: {}", hex_str),
                        position: 0,
                    })?;
                let remaining = &input[pos..];
                return if is_negative {
                    Ok((remaining, Expr::Int(-num_val)))
                } else {
                    Ok((remaining, Expr::Int(num_val)))
                };
            }
            b'o' | b'O' => {
                // Octal format: 0o77
                pos += 2; // Skip "0o"
                let oct_start = pos;
                while pos < input_bytes.len() && input_bytes[pos] >= b'0' && input_bytes[pos] <= b'7' {
                    pos += 1;
                }
                if pos == oct_start {
                    return Err(ParseError {
                        message: "Expected octal digit after 0o".to_string(),
                        position: pos,
                    });
                }
                let oct_str = &input[oct_start..pos];
                let num_val = i64::from_str_radix(oct_str, 8)
                    .map_err(|_| ParseError {
                        message: format!("Invalid octal number: {}", oct_str),
                        position: 0,
                    })?;
                let remaining = &input[pos..];
                return if is_negative {
                    Ok((remaining, Expr::Int(-num_val)))
                } else {
                    Ok((remaining, Expr::Int(num_val)))
                };
            }
            b'b' | b'B' => {
                // Binary format: 0b1010
                pos += 2; // Skip "0b"
                let bin_start = pos;
                while pos < input_bytes.len() && (input_bytes[pos] == b'0' || input_bytes[pos] == b'1') {
                    pos += 1;
                }
                if pos == bin_start {
                    return Err(ParseError {
                        message: "Expected binary digit after 0b".to_string(),
                        position: pos,
                    });
                }
                let bin_str = &input[bin_start..pos];
                let num_val = i64::from_str_radix(bin_str, 2)
                    .map_err(|_| ParseError {
                        message: format!("Invalid binary number: {}", bin_str),
                        position: 0,
                    })?;
                let remaining = &input[pos..];
                return if is_negative {
                    Ok((remaining, Expr::Int(-num_val)))
                } else {
                    Ok((remaining, Expr::Int(num_val)))
                };
            }
            _ => {} // Regular decimal starting with 0
        }
    }

    // Parse decimal integer part
    let dec_start = pos;
    while pos < input_bytes.len() && input_bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    // Check for decimal point (indicating float) or type suffix
    let is_float = pos < input_bytes.len() && input_bytes[pos] == b'.' &&
                   (pos + 1 >= input_bytes.len() || input_bytes[pos + 1].is_ascii_digit());

    let is_type_suffix = pos < input_bytes.len() && input_bytes[pos] == b'.' &&
                         pos + 1 < input_bytes.len() && input_bytes[pos + 1].is_ascii_alphabetic();

    if is_float {
        pos += 1; // Skip decimal point
        while pos < input_bytes.len() && input_bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        let num_str = &input[dec_start..pos];
        let num_val = num_str.parse::<f64>()
            .map_err(|_| ParseError {
                message: format!("Invalid float: {}", num_str),
                position: 0,
            })?;
        let final_val = if is_negative { -num_val } else { num_val };

        // Check for type suffix after float (e.g., 3.14.F32)
        let remaining = &input[pos..];
        if remaining.starts_with(".F32") {
            return Ok((&remaining[4..], Expr::Float(final_val))); // For now, just return float
        } else if remaining.starts_with(".F64") {
            return Ok((&remaining[4..], Expr::Float(final_val)));
        } else if remaining.starts_with(".Dec") {
            return Ok((&remaining[4..], Expr::Float(final_val))); // Dec treated as float for now
        }

        Ok((remaining, Expr::Float(final_val)))
    } else if is_type_suffix {
        // Parse number then type suffix: e.g., 255.U8, -128.I8
        let num_str = &input[dec_start..pos];
        let num_val = num_str.parse::<i64>()
            .map_err(|_| ParseError {
                message: format!("Invalid integer: {}", num_str),
                position: 0,
            })?;
        let final_val = if is_negative { -num_val } else { num_val };

        // Parse type suffix (letters and digits, e.g., U8, I32, F64, Dec)
        pos += 1; // Skip '.'
        let suffix_start = pos;
        while pos < input_bytes.len() && (input_bytes[pos].is_ascii_alphanumeric()) {
            pos += 1;
        }
        let _suffix = &input[suffix_start..pos];
        // Note: we ignore the suffix for now; type checker will handle it
        // We just need the parser to accept it without error

        let remaining = &input[pos..];
        Ok((remaining, Expr::Int(final_val)))
    } else {
        // Regular decimal integer
        let num_str = &input[dec_start..pos];
        let num_val = num_str.parse::<i64>()
            .map_err(|_| ParseError {
                message: format!("Invalid integer: {}", num_str),
                position: 0,
            })?;
        let final_val = if is_negative { -num_val } else { num_val };

        let remaining = &input[pos..];
        Ok((remaining, Expr::Int(final_val)))
    }
}

/// Parse identifier: x, main, birds
fn parse_identifier(input: &str) -> Result<(&str, Expr<'static>), ParseError> {
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

    // Optional trailing ! for effectful functions (e.g., echo!, main!)
    if pos < input.len() && input.as_bytes()[pos] == b'!' {
        pos += 1;
    }

    let ident = &input[..pos];
    let remaining = &input[pos..];

    Ok((remaining, Expr::Ident(string_pool::intern(ident))))
}

/// Parse string interpolation: "text ${expr} more"
/// Returns vector of literal strings and expressions
fn parse_interpolation_parts(content: &str) -> Result<Vec<StrPart<'static>>, ParseError> {
    let mut parts = Vec::new();
    let mut current_literal = String::new();
    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < bytes.len() {
        // Look for ${
        if pos + 1 < bytes.len() && bytes[pos] == b'$' && bytes[pos + 1] == b'{' {
            // Save current literal if any
            if !current_literal.is_empty() {
                parts.push(StrPart::Literal(string_pool::intern(&current_literal)));
                current_literal.clear();
            }

            // Find matching }
            pos += 2; // Skip ${
            let expr_start = pos;
            let mut brace_depth = 1;

            while pos < bytes.len() && brace_depth > 0 {
                if bytes[pos] == b'{' {
                    brace_depth += 1;
                } else if bytes[pos] == b'}' {
                    brace_depth -= 1;
                }
                pos += 1;
            }

            if brace_depth != 0 {
                return Err(ParseError {
                    message: "Unclosed ${ in string interpolation".to_string(),
                    position: 0,
                });
            }

            // Parse expression
            let expr_str = &content[expr_start..pos - 1];
            let mut expr_parser = Parser::new(expr_str);
            let expr = expr_parser.parse_expr()?;
            parts.push(StrPart::Expr(Box::leak(Box::new(expr))));
        } else {
            current_literal.push(bytes[pos] as char);
            pos += 1;
        }
    }

    // Add final literal if any
    if !current_literal.is_empty() {
        parts.push(StrPart::Literal(string_pool::intern(&current_literal)));
    }

    Ok(parts)
}

/// Check if '-' is followed by a digit (negative literal) vs subtraction operator
fn is_next_digit(rest: &str) -> bool {
    if rest.len() < 2 {
        return false;
    }
    let after_minus = &rest[1..];
    after_minus.chars().next().map_or(false, |c| c.is_ascii_digit())
}
