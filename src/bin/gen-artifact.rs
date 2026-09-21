//! Write `src/roc/Builtin.artifact`: `Builtin.roc`, parsed.
//!
//! A build script cannot call the crate it is building, so this is a binary that runs
//! against the built library and its output is checked in. `build.rs` compares the hash
//! the artifact records against `src/roc/Builtin.roc` and fails the build if they have
//! drifted, so a stale one is a compile error and never a wrong answer.
//!
//!     cargo run --release --bin gen-artifact
//!     tests/check_artifact.sh          # regenerates and diffs, for CI
use rocflight::artifact::{put_member, source_hash, Writer};

fn main() {
    let mut writer = Writer::default();
    let mut written = 0usize;
    // Every member parses the same however it was reached, EXCEPT the low-level
    // section, which is cut down to what the other selected members use. So each
    // selection is parsed and the cut-down stored under its own key.
    let mut seen: Vec<String> = Vec::new();
    for selection in rocflight::builtin::artifact_selections() {
        let parsed = rocflight::builtin::parse_members(&selection)
            .unwrap_or_else(|e| panic!("parsing {:?}: {}", selection, e));
        for (loaded, base, offsets) in &parsed {
            let key = rocflight::builtin::artifact_key_for(loaded.name, &selection);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            put_member(
                &mut writer,
                &key,
                *base,
                offsets,
                &loaded.ast,
                &loaded.intrinsics,
                &loaded.signatures,
                &loaded.nominals,
            );
            written += 1;
        }
    }
    let blob = writer.finish(source_hash(rocflight::builtin::SOURCE), written);
    let path = std::path::Path::new("src/roc/Builtin.artifact");
    std::fs::write(path, &blob).unwrap_or_else(|e| panic!("writing {}: {}", path.display(), e));
    eprintln!("wrote {} members, {} bytes to {}", written, blob.len(), path.display());
}
