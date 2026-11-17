use super::{Param, runs::ListQuery};
use axum::{extract::FromRequestParts, extract::Query, http::Request, response::IntoResponse};

#[tokio::test]
async fn a_malformed_query_is_an_error_reply() {
    let (mut parts, ()) = Request::get("/v1/runs?limit=many").body(()).unwrap().into_parts();
    let Err(e) = Param::<Query<ListQuery>>::from_request_parts(&mut parts, &()).await else {
        panic!("`limit=many` is accepted");
    };
    let reply = e.into_response();
    assert_eq!(reply.status(), 400);
    let body = axum::body::to_bytes(reply.into_body(), 1 << 16).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], "bad_request", "{body}");
}

/// `explain --formula` names the layer of `rounds` and of each input value the config gives (docs/design/config.md).
#[test]
fn a_formula_is_explained_with_the_layers_of_its_rounds_and_input_values() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "config.toml",
            "projects = [\"shop\"]\nrounds = 2\n[inputs]\nfocus = \"top\"\nn = 1\n",
        ),
        ("shop/aae.toml", "[inputs]\nn = \"4\"\n"),
        (
            "shop/formulas/flow.toml",
            "name = \"flow\"\n[inputs]\nfocus = { type = \"string\" }\nn = { type = \"integer\" }\n\
             [[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        ),
    ];
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let config = aae_core::config::load(&dir.path().join("config.toml")).unwrap();
    let of = super::config::Explain::Formula("flow".into());
    let reply = super::config::explained(&config, "shop", &of, None).unwrap();
    let text = reply["text"].as_str().unwrap();
    let head = "rounds = 2  # orchestrator\ninputs.focus = \"top\"  # orchestrator\ninputs.n = 4  # project\n";
    assert!(text.starts_with(head), "{text}");
}
