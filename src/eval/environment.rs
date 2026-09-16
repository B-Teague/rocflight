//! Stack-based environment for variable bindings

use crate::eval::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Stack frame for scope
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub vars: Vec<(&'static str, Value)>,
}

/// One scope, shared rather than copied. See `Environment`.
type Scope = Rc<RefCell<StackFrame>>;

/// Environment with scope stack.
///
/// The outermost scope is SHARED between clones. roc's top level is order-independent
/// — a function may call one declared further down the file, and two may call each
/// other — but a closure captures its environment by value, so a deep copy would
/// freeze the top level as it stood when the closure was built. Sharing it means every
/// closure sees every top-level name, whenever it was bound.
///
/// Every INNER scope is shared too, and for a different reason: speed. A closure
/// captures its environment, and `apply` clones that environment on every call. When
/// the frames were owned, each call copied every binding in them — so a lambda written
/// beside a 4000-element list copied the list 4000 times, and the same map-and-fold ran
/// 129x slower than one whose lambda captured nothing. Holding each frame behind an
/// `Rc` makes the clone a handful of refcount bumps whatever the frames contain.
///
/// Sharing a frame means a later binding in it is visible to a closure made earlier.
/// That matches roc: a plain name cannot be rebound in a scope (roc calls it a
/// duplicate definition), and a `var` that is reassigned is *meant* to be seen.
#[derive(Debug, Clone)]
pub struct Environment {
    top: Rc<RefCell<Vec<(&'static str, Value)>>>,
    /// Scopes inside the top level: call frames, blocks, loop bodies. The Vec is
    /// cloned — it is one pointer per nesting level — but the frames are not.
    scopes: Vec<Scope>,
}

impl Environment {
    /// Create new environment
    pub fn new() -> Self {
        Environment {
            top: Rc::new(RefCell::new(Vec::new())),
            scopes: Vec::new(),
        }
    }

    /// Push new scope
    pub fn push_scope(&mut self) {
        self.scopes.push(Rc::new(RefCell::new(StackFrame { vars: Vec::new() })));
    }

    /// Pop scope
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Bind variable in current scope
    pub fn bind(&mut self, name: &'static str, value: Value) {
        match self.scopes.last() {
            Some(frame) => frame.borrow_mut().vars.push((name, value)),
            // At the top level, so the binding is shared with every closure.
            None => self.top.borrow_mut().push((name, value)),
        }
    }

    /// Update an existing binding in place, innermost scope first.
    ///
    /// Returns whether the name was found. This is what makes a `var` mutable: a loop
    /// body runs in its own scope, so binding there would shadow rather than update,
    /// and the value read after the loop would be the original.
    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter().rev() {
            for (var_name, slot) in scope.borrow_mut().vars.iter_mut().rev() {
                if *var_name == name {
                    *slot = value;
                    return true;
                }
            }
        }
        for (var_name, slot) in self.top.borrow_mut().iter_mut().rev() {
            if *var_name == name {
                *slot = value;
                return true;
            }
        }
        false
    }

    /// Find nominal methods named `method`, as `(qualified name, value)`.
    ///
    /// A nominal's methods are bound as `Type.method`. At run time a value carries no
    /// nominal tag — roc erases it, which `Str.inspect` showing the bare backing record
    /// confirms — so dispatch cannot know which type a record belongs to and searches
    /// by method name instead.
    ///
    /// ponytail: ambiguous when two nominals declare the same method name; the caller
    /// reports that rather than guessing. Resolving it properly needs the checker to
    /// record the method at each call site, which it already knows.
    pub fn methods_named(&self, method: &str) -> Vec<(&'static str, Value)> {
        let suffix = format!(".{}", method);
        let mut found: Vec<(&'static str, Value)> = Vec::new();
        let mut consider = |name: &'static str, value: &Value| {
            if name.ends_with(&suffix)
                // A VM closure counts: a nominal's method block is dispatched through
                // here, and under the VM its methods are `Closure`s. Checking only for
                // `Lambda` made `a + b` on a type that defines `plus` silently fall
                // through to the built-in operator.
                && matches!(value, Value::Lambda(..) | Value::Closure(..))
                && !found.iter().any(|(seen, _)| *seen == name)
            {
                found.push((name, value.clone()));
            }
        };
        for scope in self.scopes.iter().rev() {
            for (name, value) in scope.borrow().vars.iter().rev() {
                consider(name, value);
            }
        }
        for (name, value) in self.top.borrow().iter().rev() {
            consider(name, value);
        }
        found
    }

    /// Look up variable (searches from top scope downward)
    pub fn lookup(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            for (var_name, value) in scope.borrow().vars.iter().rev() {
                if *var_name == name {
                    return Some(value.clone());
                }
            }
        }
        for (var_name, value) in self.top.borrow().iter().rev() {
            if *var_name == name {
                return Some(value.clone());
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
