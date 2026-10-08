mod build_support;
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let identity = build_support::source_identity(&root);
    for path in &identity.git_paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for path in [
        "build.rs",
        "build_support.rs",
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "schemas",
        "tests",
        "config",
        "README.md",
        ".changeset",
        ".claude",
        ".github",
        "scripts",
        "examples",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let revision = identity.revision.as_deref().unwrap_or("");
    println!("cargo:rustc-env=LOG_ANALYZER_REVISION={revision}");
    println!(
        "cargo:rustc-env=LOG_ANALYZER_BUILD_STATE={}",
        identity.state
    );
    let detail = identity
        .revision
        .as_ref()
        .map(|revision| format!("{} {}", &revision[..7], identity.state))
        .unwrap_or_else(|| "unknown".into());
    println!(
        "cargo:rustc-env=LOG_ANALYZER_BUILD_VERSION={} ({detail})",
        std::env::var("CARGO_PKG_VERSION").unwrap()
    );
}
