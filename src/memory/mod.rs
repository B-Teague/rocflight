//! Memory management: arena allocation and string interning
//!
//! Phase 1: String interning

pub mod string_pool;

// `string_pool::intern` is the whole of the interning API; the pool itself is private.
