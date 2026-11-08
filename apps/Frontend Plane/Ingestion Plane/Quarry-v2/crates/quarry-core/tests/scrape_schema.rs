//! The JSON Schema of `ScrapeOptions`, pinned as a golden.
//!
//! `docs/schemas/scrape-options.schema.json` is derived from the Rust type, so it
//! cannot drift from what the edge parses: this test fails when the type changes
//! without the golden, and `UPDATE_SCHEMAS=1` rewrites it. The OpenAPI file is
//! checked against the golden by `scripts/tests/openapi-scrape-parity.test.mjs`,
//! and the Model Plane vendors the golden as the scrape tool's argument schema.
//!
//! Run with `cargo test -p quarry-core --features jsonschema`.

use quarry_core::scrape::{EffectiveScrapeOptions, ScrapeOptions};

fn golden_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schemas").join(name)
}

fn check(name: &str, schema: schemars::Schema) {
    let rendered = serde_json::to_string_pretty(&schema).expect("schema serializes") + "\n";
    let path = golden_path(name);
    if std::env::var("UPDATE_SCHEMAS").as_deref() == Ok("1") {
        std::fs::write(&path, &rendered).expect("write golden");
        return;
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing: run with UPDATE_SCHEMAS=1", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        golden, rendered,
        "{name} no longer matches the Rust type: rerun with UPDATE_SCHEMAS=1 and review the diff"
    );
}

#[test]
fn the_scrape_options_schema_matches_its_golden() {
    check("scrape-options.schema.json", schemars::schema_for!(ScrapeOptions));
}

#[test]
fn the_effective_options_schema_matches_its_golden() {
    check(
        "effective-scrape-options.schema.json",
        schemars::schema_for!(EffectiveScrapeOptions),
    );
}
