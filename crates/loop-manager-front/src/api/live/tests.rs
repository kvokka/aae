use super::*;
use http_body_util::BodyExt;

#[tokio::test]
async fn a_reader_behind_the_session_gets_lagged_with_the_missed_count_then_the_rest() {
    let (sender, receiver) = broadcast::channel(2);
    for n in 0..5 {
        sender.send(json!({ "n": n })).unwrap();
    }
    drop(sender);
    let mut tap = Some(receiver);
    let mut events = Vec::new();
    while let Some(event) = tapped(&mut tap).await {
        events.push(Ok::<_, Infallible>(event));
    }
    assert!(tap.is_none(), "the ended session is no longer tapped");
    let body = Sse::new(tokio_stream::iter(events)).into_response().into_body();
    let text = String::from_utf8(body.collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert_eq!(
        text,
        "event: lagged\ndata: {\"missed\":3}\n\n\
         event: harness\ndata: {\"n\":3}\n\n\
         event: harness\ndata: {\"n\":4}\n\n"
    );
}
