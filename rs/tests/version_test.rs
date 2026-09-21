// The baked-in VERSION must equal the crate's declared version and the
// canonical package version.
//
// This is the CI check for version drift. It exists because the constant
// HAS drifted in practice: jsonic-cli shipped 0.4.1 and 0.4.2 while its
// constant sat at 0.4.0, and @tabnas/json shipped a TypeScript `Version`
// export reading 1.0.0 for several releases because nothing ever rewrote
// it. Both were invisible until someone read the file. A release that
// bumps one site and forgets another now fails here instead of shipping a
// lie.

use std::fs;
use std::path::Path;

mod common;

use common::repo_root;

/// Read a `"version": "x.y.z"` field out of a JSON file without a JSON
/// dependency in the crate itself.
fn json_version(text: &str) -> Option<String> {
    let at = text.find("\"version\"")?;
    let rest = &text[at + "\"version\"".len()..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let open = rest.find('"')?;
    let rest = &rest[open + 1..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

fn cargo_version(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_string())
}

#[test]
fn version_matches_package_json() {
    // Deliberately fatal, never skipped: a version check that silently
    // does not run is the failure mode this test exists to prevent.
    let package = fs::read_to_string(repo_root().join("ts").join("package.json"))
        .expect("ts/package.json is readable, or VERSION cannot be checked");
    let declared = json_version(&package).expect("ts/package.json has a version field");
    assert_eq!(
        tabnas_expr::VERSION,
        declared,
        "VERSION drift: the crate exports {} but ts/package.json is {declared}. Both are \
         rewritten by the release orchestrator; if you bumped one by hand, bump the other.",
        tabnas_expr::VERSION
    );
}

#[test]
fn version_matches_cargo_toml() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("rs/Cargo.toml is readable");
    let declared = cargo_version(&manifest).expect("rs/Cargo.toml has a version field");
    assert_eq!(tabnas_expr::VERSION, declared);
}

#[test]
fn version_is_semver() {
    let parts: Vec<&str> = tabnas_expr::VERSION.split('.').collect();
    assert!(3 <= parts.len(), "VERSION must be a semver");
    for part in parts.iter().take(3) {
        assert!(
            part.chars().all(|digit| digit.is_ascii_digit()),
            "VERSION must be a semver"
        );
    }
}
