//! The published JSON Schema (`schema/ovl.schema.json`) is generated from the model.
use std::path::Path;

fn generated() -> String {
    serde_json::to_string_pretty(&actionlay_layout::json_schema()).unwrap() + "\n"
}

#[test]
fn committed_schema_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/ovl.schema.json");
    let generated = generated();
    if std::env::var_os("ACTIONLAY_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run with ACTIONLAY_UPDATE_SCHEMA=1",
            path.display()
        )
    });
    // Windows checkouts may convert line endings
    assert!(
        committed.replace("\r\n", "\n") == generated,
        "schema/ovl.schema.json is stale: run `ACTIONLAY_UPDATE_SCHEMA=1 cargo test -p actionlay-layout --test schema`"
    );
}

#[test]
fn default_layout_validates_against_the_schema() {
    let schema = actionlay_layout::json_schema();
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    let instance: serde_json::Value =
        serde_json::from_str(actionlay_layout::DEFAULT_LAYOUT_JSON).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn schema_rejects_bad_values() {
    let schema = actionlay_layout::json_schema();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for bad in [
        serde_json::json!({"version": 1, "nodes": [{"type": "text", "text": "x", "anchor": "middle"}]}),
        serde_json::json!({"version": 1, "nodes": [{"type": "text", "text": "x", "color": "blue"}]}),
        serde_json::json!({"version": 1, "nodes": [{"type": "frame"}]}),
        serde_json::json!({"nodes": []}),
    ] {
        assert!(!validator.is_valid(&bad), "accepted: {bad}");
    }
}
