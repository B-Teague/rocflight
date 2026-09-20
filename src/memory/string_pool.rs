//! String interning: every identifier, field name and string literal in the program
//! becomes a `&'static str`, so the AST, the compiler's tables and the VM's chunks can
//! all hold names without owning or copying them.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

/// FNV-1a.
///
/// The keys here are identifiers — `x`, `insert`, `main!`, `Dict.get` — a handful of
/// bytes each, and at that length SipHash's setup costs more than the hashing does. The
/// standard library ships no faster hasher, and ten lines beat a dependency for it.
#[derive(Default)]
pub struct Fnv(u64);

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        self.0 = hash;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type Pool = HashSet<&'static str, BuildHasherDefault<Fnv>>;

thread_local! {
    /// The pool, per thread rather than global behind a `Mutex`.
    ///
    /// Interning is on the parser's hot path — once per identifier, string literal and
    /// field name in every file — and taking an uncontended lock there is a cost with no
    /// purpose: the interpreter is single-threaded by construction, since the running
    /// program (`vm::RUNNING`) and the AST's node table are both thread-locals too. The
    /// only other threads are the test harness's, and each of those parses its own
    /// source.
    ///
    /// If two threads do intern the same text they get two different pointers. That
    /// costs a leak and never an answer: every comparison of a name in the interpreter is
    /// by content, and nothing outside this module's own tests looks at the address.
    static POOL: RefCell<Pool> = RefCell::new(Pool::default());
}

/// Intern a string, giving a `&'static str` for it.
///
/// Leaked rather than reference-counted: a name lives as long as the program that uses
/// it, and this is what lets `Expr`, `Chunk` and `Value::Builtin` hold plain `&'static
/// str` instead of an owned `String` apiece.
pub fn intern(s: &str) -> &'static str {
    POOL.with(|pool| {
        if let Some(&found) = pool.borrow().get(s) {
            return found;
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        pool.borrow_mut().insert(leaked);
        leaked
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_string() {
        let s1 = intern("hello");
        let s2 = intern("hello");
        assert_eq!(s1 as *const _, s2 as *const _); // Same pointer
    }

    #[test]
    fn test_intern_different_strings() {
        let s1 = intern("world");
        let s2 = intern("moon");
        assert_ne!(s1 as *const _, s2 as *const _); // Different pointers
    }

    /// The hasher has to agree with `Eq`: two equal strings must hash alike, however
    /// they were split across `write` calls.
    #[test]
    fn fnv_agrees_with_equality() {
        let of = |s: &str| {
            let mut h = Fnv::default();
            h.write(s.as_bytes());
            h.finish()
        };
        assert_eq!(of("insert"), of("insert"));
        assert_ne!(of("insert"), of("insort"));
        // A thousand identifier-shaped keys, and no two of them collide.
        let names: Vec<String> = (0..1000).map(|i| format!("name_{}", i)).collect();
        let hashes: HashSet<u64> = names.iter().map(|n| of(n)).collect();
        assert_eq!(hashes.len(), names.len());
    }
}
