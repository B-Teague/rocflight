//! Real platform loading — phase 19.
//!
//! Verified against `roc` nightly-2026-09-03 and basic-cli 0.22.0:
//!   * `roc` caches dependencies content-addressed at
//!     `~/.cache/roc/packages/<HASH>/`, where `<HASH>` is the URL's filename with its
//!     archive extension removed — so a URL maps to a directory with no network access
//!   * the archive is `.tar.zst` now; the Rust-era compiler used `.tar.br`
//!   * a platform root declares `requires`, `exposes`, `packages`, `provides`, `hosted`
//!   * an exposed module declares its members inside `Name :: [].{ ... }`; anything
//!     after that block is private
//!
//! The architectural limit: a platform's `hosted` functions live in its COMPILED HOST,
//! which a tree-walking interpreter cannot call. Effects run only where this
//! interpreter supplies its own implementation; the rest are reported as a gap.

use rocflight::parser::Parser;
use rocflight::platform::{real, resolve};

/// basic-cli 0.22.0 — the platform the roc-lang example uses.
const CLI: &str = "https://github.com/roc-lang/basic-cli/releases/download/0.22.0/F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL.tar.zst";

fn cached() -> bool {
    resolve::is_cached(CLI)
}

// --- URL resolution (no cache needed) -------------------------------------

#[test]
fn a_url_maps_to_a_cache_directory_by_its_hash() {
    assert_eq!(resolve::hash_from_url(CLI), Some("F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL"));
}

#[test]
fn both_archive_formats_are_recognised() {
    // `.tar.zst` is current; `.tar.br` is what the Rust-era compiler produced.
    assert_eq!(resolve::hash_from_url("https://x/y/AAA.tar.zst"), Some("AAA"));
    assert_eq!(resolve::hash_from_url("https://x/y/BBB.tar.br"), Some("BBB"));
}

#[test]
fn a_compiler_version_pin_is_not_a_fetchable_dependency() {
    // `roc: "nightly-2026-09-03-62fcb65"` names no archive.
    assert_eq!(resolve::hash_from_url("nightly-2026-09-03-62fcb65"), None);
}

// --- app header parsing ---------------------------------------------------

#[test]
fn the_app_header_records_platform_and_package_dependencies() {
    let src = "app [main!] {\n\tcli: platform \"https://x/A.tar.zst\",\n\tunicode: \"https://y/B.tar.zst\",\n\troc: \"nightly-2026-09-03\",\n}\n\nmain! = |_a| Ok({})\n";
    let mut parser = Parser::new(src);
    let _ = parser.parse_expr();

    let deps = parser.dependencies();
    assert_eq!(deps.len(), 3);
    assert_eq!(deps[0], ("cli".into(), "https://x/A.tar.zst".into(), true));
    assert_eq!(deps[1], ("unicode".into(), "https://y/B.tar.zst".into(), false));
    // The compiler pin is recorded like any other and filtered out by the loader.
    assert_eq!(deps[2], ("roc".into(), "nightly-2026-09-03".into(), false));
}

#[test]
fn imports_are_recorded_with_their_dependency_alias() {
    let src = "app [main!] { cli: platform \"https://x/A.tar.zst\" }\n\nimport cli.Stdout\nimport cli.Env exposing [var]\nimport Local\n\nmain! = |_a| Ok({})\n";
    let mut parser = Parser::new(src);
    let _ = parser.parse_expr();

    // A bare `import Local` has no alias: it is a file beside the app.
    assert_eq!(
        parser.imports(),
        &[
            ("cli".to_string(), "Stdout".to_string()),
            ("cli".to_string(), "Env".to_string()),
            (String::new(), "Local".to_string()),
        ]
    );
}

#[test]
fn the_first_import_survives_the_dependency_map() {
    // Regression: after parsing the header's `{ ... }` the cursor already sat on the
    // next line, and skipping to the next line again swallowed the first `import`.
    let src = "app [main!] { cli: platform \"https://x/A.tar.zst\" }\n\nimport cli.Stdout\n\nmain! = |_a| Ok({})\n";
    let mut parser = Parser::new(src);
    let _ = parser.parse_expr();
    assert_eq!(parser.imports().len(), 1, "the first import was dropped");
}

// --- reading a real platform (needs the cache) ----------------------------

#[test]
fn a_real_platform_root_is_read() {
    if !cached() {
        eprintln!("skipped: run `roc check` on tests/roc/19_platform/basic_cli.roc once");
        return;
    }
    let platform = real::load("cli", CLI).expect("should load");

    assert!(platform.exposes_module("Stdout"), "basic-cli exposes Stdout");
    assert!(platform.exposes.len() > 10, "exposes many modules");
    assert!(!platform.hosted.is_empty(), "declares hosted effects");

    // The `requires` signature contains `{}`, so a scan to the first `}` truncates it.
    let requires = platform.requires.as_deref().expect("requires clause");
    assert!(requires.starts_with("main! :"), "got {}", requires);
    assert!(requires.contains("Try("), "got {}", requires);
}

#[test]
fn an_exposed_modules_members_are_read_with_their_signatures() {
    if !cached() {
        return;
    }
    let platform = real::load("cli", CLI).expect("should load");
    let members = platform.read_module("Stdout").expect("Stdout is exposed");

    let line = members.iter().find(|m| m.name == "line!").expect("line! is declared");
    assert!(
        line.signature.starts_with("Str =>"),
        "the signature comes from the platform's own source, got {}",
        line.signature
    );
}

#[test]
fn a_private_helper_is_not_a_member() {
    if !cached() {
        return;
    }
    // Stdout.roc closes its method block and then defines `widen_stdout_err`.
    let platform = real::load("cli", CLI).expect("should load");
    let members = platform.read_module("Stdout").expect("Stdout is exposed");
    assert!(
        !members.iter().any(|m| m.name == "widen_stdout_err"),
        "a definition after the method block is private, got {:?}",
        members.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
}

#[test]
fn importing_an_unexposed_module_names_what_is_available() {
    if !cached() {
        return;
    }
    let platform = real::load("cli", CLI).expect("should load");
    let err = platform.read_module("Nope").expect_err("Nope is not exposed");
    assert!(err.contains("does not expose"), "got {}", err);
    assert!(err.contains("Stdout"), "the message should list what IS exposed: {}", err);
}

// --- failure modes --------------------------------------------------------

#[test]
fn an_unfetched_platform_says_how_to_fetch_it() {
    let err = real::load(
        "cli",
        "https://github.com/roc-lang/basic-cli/releases/download/9.9.9/NOTFETCHED.tar.zst",
    )
    .expect_err("not cached");
    assert!(err.contains("roc check"), "the error should name the fix: {}", err);
}

#[test]
fn an_import_naming_no_declared_dependency_is_rejected() {
    let deps = vec![("cli".to_string(), CLI.to_string(), true)];
    let imports = vec![("other".to_string(), "Thing".to_string())];
    let err = real::verify_app(&deps, &imports).expect_err("`other` was never declared");
    assert!(err.contains("no declared dependency"), "got {}", err);
}

#[test]
fn a_compiler_pin_is_skipped_rather_than_fetched() {
    // `roc: "nightly-..."` must not be treated as a platform to load.
    let deps = vec![("roc".to_string(), "nightly-2026-09-03".to_string(), false)];
    assert!(real::verify_app(&deps, &[]).is_ok());
}
