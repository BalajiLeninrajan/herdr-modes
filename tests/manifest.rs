//! herdr-plugin.toml is what herdr reads; Cargo.toml is what cargo reads.
//! Nothing ties the two together except this test.

use std::path::Path;

fn manifest() -> toml::Table {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-plugin.toml");
    let text = std::fs::read_to_string(&path).expect("read herdr-plugin.toml");
    text.parse().expect("parse herdr-plugin.toml")
}

#[test]
fn plugin_version_matches_cargo() {
    let m = manifest();
    assert_eq!(
        m["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "herdr-plugin.toml version must equal Cargo.toml version"
    );
}

#[test]
fn plugin_declares_min_herdr_version() {
    let m = manifest();
    let v = m
        .get("min_herdr_version")
        .and_then(|v| v.as_str())
        .expect("herdr-plugin.toml must set min_herdr_version");
    assert!(!v.is_empty());
}
