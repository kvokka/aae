//! The committed OpenAPI document is the one the code generates (spec §12, criterion 17).

use std::{fs, path::Path};

#[test]
fn the_committed_openapi_document_matches_the_code() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
    let generated = aae_front::api::openapi().to_pretty_json().unwrap() + "\n";
    if std::env::var_os("AAE_UPDATE_OPENAPI").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, generated).unwrap();
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "docs/api/openapi.json differs from the code; regenerate it with \
         `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi` and commit it"
    );
}
