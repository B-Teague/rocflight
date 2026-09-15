//! Effects provided by the default (platformless) host.
//!
//! `roc run file.roc` with no platform in the app header links a built-in host.
//! That host — not the compiler — supplies `echo!`; it is NOT a global builtin
//! and is out of scope in a type module. Signature confirmed against
//! `roc experimental-lsp` hover on nightly-2026-09-03: `echo! : Str => {}`.
//!
//! `=>` (not `->`) is the effectful arrow. The trailing `!` is part of the
//! identifier, so these names are looked up verbatim.

/// Host effects and their signatures, as `(name, arg types, return type)`.
///
/// ponytail: a flat table is enough while the default host has one effect.
/// When real platforms load (see the platform-loading phase), the platform's
/// own `hosted` block replaces this.
pub const HOST_EFFECTS: &[(&str, &[&str], &str)] = &[("echo!", &["Str"], "{}")];

/// Look up a host effect's signature by its exact name (`!` included).
pub fn lookup(name: &str) -> Option<(&'static [&'static str], &'static str)> {
    HOST_EFFECTS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, args, ret)| (*args, *ret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_matches_lsp_signature() {
        // `roc experimental-lsp` hover reports `Str => {}` for echo!.
        assert_eq!(lookup("echo!"), Some((&["Str"][..], "{}")));
    }

    #[test]
    fn bang_is_part_of_the_name() {
        // `echo` without the `!` is a different, undefined name.
        assert!(lookup("echo").is_none());
    }
}
