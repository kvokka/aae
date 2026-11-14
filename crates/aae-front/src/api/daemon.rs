//! The daemon (spec §12, §9.3): its status, and a drain or a planned restart — no run starts from then on, the
//! daemon stops when the runs in a turn are done or the drain times out, and a restart starts it again in place.

use super::{Reply, State, key};
use aae_core::{Clock, run::Msg};
use axum::{Json, extract::Query, http::HeaderMap};
use serde::Deserialize;
use serde_json::json;
use std::time::{Duration, Instant};
use utoipa::IntoParams;

/// The daemon: its version, process and socket, the journal (last `seq`, failed writes in a row and the last
/// error), the outbox (depth and age of the oldest row, contracts 3.5), the runs in a turn and whether it stops.
#[utoipa::path(get, path = "/v1/daemon", responses((status = OK, body = Object)))]
pub(crate) async fn status(state: State) -> Reply {
    let mut daemon = state.daemon.clone();
    let mut status = state.reader.status()?;
    let oldest = status["outbox_oldest_ms"].take().as_i64();
    daemon["journal_seq"] = json!(*state.written.borrow());
    daemon["journal"] = json!(*state.journal_health.borrow());
    daemon["outbox_depth"] = status["outbox_depth"].take();
    daemon["outbox_oldest_age_ms"] = json!(oldest.map(|t| Clock::system().now_ms() - t));
    daemon["active_runs"] = status["active_runs"].take();
    daemon["stopping"] = json!(*state.stop.borrow());
    Ok(Json(daemon))
}

#[derive(Deserialize, IntoParams)]
pub(crate) struct Drain {
    /// How long the runs in a turn may go on, in seconds (default 300); the daemon stops then anyway.
    timeout_s: Option<u64>,
}

/// Drains the daemon: no run starts from now on (`daemon.stopping{reason: "drain"}`), and it exits once no run
/// is in a turn or after `timeout_s`. The reply is `{"seq": <daemon.stopping>, "reason", "active_runs": n}`; a drain
/// asked while a restart is under way, or the reverse, is refused with 409.
#[utoipa::path(post, path = "/v1/daemon/drain", params(Drain, ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn drain(state: State, headers: HeaderMap, Query(q): Query<Drain>) -> Reply {
    stop(state, &headers, q, "drain").await
}

/// A drain after which the daemon starts again in the same process (`daemon.stopping{reason: "restart"}`);
/// recovery resumes the runs the drain cut (spec §5.6).
#[utoipa::path(post, path = "/v1/daemon/restart", params(Drain, ("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn restart(state: State, headers: HeaderMap, Query(q): Query<Drain>) -> Reply {
    stop(state, &headers, q, "restart").await
}

async fn stop(state: State, headers: &HeaderMap, q: Drain, reason: &'static str) -> Reply {
    let msg = Msg::Stopping {
        reason: reason.to_owned(),
    };
    let mut reply = state.core.send_request(key(headers), msg).await?;
    // The first reason wins: a restart asked during a drain does not turn it into one, nor the reverse.
    if reply["reason"] != reason {
        return Err(aae_core::Error::Rejected(format!("the daemon is already stopping: {}", reply["reason"])).into());
    }
    reply["active_runs"] = state.reader.status()?["active_runs"].take();
    let deadline = Instant::now() + Duration::from_secs(q.timeout_s.unwrap_or(300));
    tokio::spawn(async move {
        // A failed read stops at once: the drain must end, and the state stays consistent either way.
        while Instant::now() < deadline
            && state
                .reader
                .status()
                .is_ok_and(|s| s["active_runs"].as_i64() != Some(0))
        {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        state
            .stop
            .send_if_modified(|r| r.is_none() && r.replace(reason).is_none());
    });
    Ok(Json(reply))
}
