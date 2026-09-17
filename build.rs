// The release binary carries `librocflight_host.a` — the interpreter built as a
// platform's `app`, for x86_64-unknown-linux-musl — so `rocflight main.roc` on a
// platform app works from a copied binary with nothing beside it. The driver
// extracts it into the cache on first use, the way `roc` extracts its own shim
// libraries (PLATFORM_HOST_PLAN.md §3).
//
// Build order is therefore: the host library first, then this binary:
//     cargo build -p rocflight-host --release --target x86_64-unknown-linux-musl
//     cargo build --release
// A build without the library still succeeds — tests, CI, a first build — and the
// driver then looks beside the binary or at ROCFLIGHT_LIB, and says so.
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let lib = std::env::var_os("ROCFLIGHT_HOST_LIB")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("target/x86_64-unknown-linux-musl/release/librocflight_host.a"));
    println!("cargo:rerun-if-env-changed=ROCFLIGHT_HOST_LIB");
    println!("cargo:rerun-if-changed={}", lib.display());
    let bytes = std::fs::read(&lib).unwrap_or_default();
    std::fs::write(out.join("librocflight_host.a"), bytes).expect("write embedded host library");
}
