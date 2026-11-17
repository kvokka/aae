//! The committed OpenAPI document is the one the code generates (docs/api/frontends.md, criterion 17).

use std::{fs, path::Path};

#[test]
fn the_committed_openapi_document_matches_the_code() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
    let generated = loop_manager_front::api::openapi().to_pretty_json().unwrap() + "\n";
    if std::env::var_os("LOOP_MANAGER_UPDATE_OPENAPI").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, generated).unwrap();
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "docs/api/openapi.json differs from the code; regenerate it with \
         `LOOP_MANAGER_UPDATE_OPENAPI=1 cargo nextest run -p loop-manager-front openapi` and commit it"
    );
}

#[test]
fn every_api_operation_has_a_cli_command() {
    let doc = serde_json::to_value(loop_manager_front::api::openapi()).unwrap();
    let mut api: Vec<(String, String)> = doc["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, item)| {
            item.as_object()
                .unwrap()
                .keys()
                .map(move |m| (m.to_uppercase(), path.clone()))
        })
        .collect();
    let mut cli: Vec<(String, String)> = loop_manager_front::cli::OPERATIONS
        .iter()
        .map(|(command, method, path)| {
            assert!(
                loop_manager_front::cli::has_command(command),
                "no command `loop-manager {command}`"
            );
            ((*method).to_owned(), (*path).to_owned())
        })
        .collect();
    api.sort();
    cli.sort();
    assert_eq!(api, cli, "the CLI covers exactly the operations of the API");
}

#[test]
fn every_operation_lists_the_errors_it_can_return() {
    let doc = serde_json::to_value(loop_manager_front::api::openapi()).unwrap();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            let params = op["parameters"].as_array().cloned().unwrap_or_default();
            let has = |name: &str, place: &str| {
                params
                    .iter()
                    .any(|p| p["in"] == place && (name.is_empty() || p["name"] == name))
            };
            let statuses: Vec<&String> = op["responses"].as_object().unwrap().keys().collect();
            let lists = |status: &str| statuses.iter().any(|s| *s == status);
            let at = format!("{} {path}", method.to_uppercase());
            assert!(lists("401"), "{at}: lists 401, the reply over TCP without the token");
            if has("Idempotency-Key", "header") {
                assert!(
                    lists("422") && lists("503"),
                    "{at}: a keyed operation lists 422 and 503"
                );
            }
            // A missing run or instance is 404 on any method; the finisher of one answers `invalid` instead.
            if path.contains("{id}") && !path.ends_with("/finish") {
                assert!(lists("404"), "{at}: lists 404");
            }
            if !op["requestBody"].is_null() || has("", "query") || has("", "path") {
                assert!(lists("400"), "{at}: lists 400");
            }
            for (status, reply) in op["responses"].as_object().unwrap() {
                if !status.starts_with('2') {
                    let schema = &reply["content"]["application/json"]["schema"]["$ref"];
                    assert_eq!(schema, "#/components/schemas/ErrorReply", "{at}: {status}");
                }
            }
        }
    }
}

#[test]
fn the_path_parameters_of_each_operation_are_the_names_in_its_path() {
    let doc = serde_json::to_value(loop_manager_front::api::openapi()).unwrap();
    for (path, item) in doc["paths"].as_object().unwrap() {
        let mut named: Vec<&str> = path
            .split('{')
            .skip(1)
            .filter_map(|s| s.split_once('}'))
            .map(|(n, _)| n)
            .collect();
        named.sort_unstable();
        for (method, op) in item.as_object().unwrap() {
            let params = op["parameters"].as_array().cloned().unwrap_or_default();
            let mut declared: Vec<String> = params
                .iter()
                .filter(|p| p["in"] == "path")
                .filter_map(|p| p["name"].as_str().map(str::to_owned))
                .collect();
            declared.sort_unstable();
            assert_eq!(declared, named, "{} {path}", method.to_uppercase());
        }
    }
}
