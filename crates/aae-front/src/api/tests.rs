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
