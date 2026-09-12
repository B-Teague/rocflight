//! Stack-based environment for variable bindings

use crate::eval::Value;

/// Stack frame for scope
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub vars: Vec<(&'static str, Value)>,
}

/// Environment with scope stack
#[derive(Debug, Clone)]
pub struct Environment {
    scopes: Vec<StackFrame>,
}

impl Environment {
    /// Create new environment
    pub fn new() -> Self {
        Environment {
            scopes: vec![StackFrame { vars: vec![] }],
        }
    }

    /// Push new scope
    pub fn push_scope(&mut self) {
        self.scopes.push(StackFrame { vars: vec![] });
    }

    /// Pop scope
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Bind variable in current scope
    pub fn bind(&mut self, name: &'static str, value: Value) {
        if let Some(frame) = self.scopes.last_mut() {
            frame.vars.push((name, value));
        }
    }

    /// Look up variable (searches from top scope downward)
    pub fn lookup(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            for (var_name, value) in scope.vars.iter().rev() {
                if *var_name == name {
                    return Some(value.clone());
                }
            }
        }
        None
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}
