use serde_json::json;

/// The request fingerprint of a keyed call names its tool: one key on two tools with the same arguments is a
/// key reused with another request.
#[test]
fn a_keyed_call_is_fingerprinted_with_its_tool() {
    let hash = |tool| {
        let mut args = json!({ "run": "run_1", "idempotency_key": "k-1" });
        let Ok(headers) = super::keyed(&mut args, tool) else {
            panic!("a valid key")
        };
        headers["aae-request"].clone()
    };
    assert_ne!(hash("cancel_run"), hash("start_run"));
    assert_eq!(hash("cancel_run"), hash("cancel_run"));
}
