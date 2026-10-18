//! Validation against the official UCP `2026-08-25` JSON Schemas vendored under
//! `tests/fixtures/ucp/2026-08-25/schemas`, without touching the network.

use std::path::Path;

pub const SCHEMA_BASE: &str = "https://ucp.dev/2026-08-25/schemas/";

/// Serves `https://ucp.dev/2026-08-25/schemas/...` from the vendored copies.
struct VendoredSchemas;

impl jsonschema::Retrieve for VendoredSchemas {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let relative = uri
            .as_str()
            .strip_prefix(SCHEMA_BASE)
            .ok_or_else(|| format!("unexpected schema reference {uri}"))?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ucp/2026-08-25/schemas")
            .join(relative);
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

/// Panic with every violation when `instance` does not match the schema at
/// `reference`, e.g. `profile.json#/$defs/business_schema`.
pub fn assert_valid(reference: &str, instance: &serde_json::Value) {
    let validator = jsonschema::options()
        .with_retriever(VendoredSchemas)
        .build(&serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$ref": format!("{SCHEMA_BASE}{reference}"),
        }))
        .unwrap_or_else(|e| panic!("UCP schema {reference} compiles: {e}"));
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "instance violates UCP 2026-08-25 {reference}:\n{}\n{instance:#}",
        errors.join("\n")
    );
}
