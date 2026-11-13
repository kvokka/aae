//! The HTTP API (spec §12): JSON over a unix socket, the event stream as SSE. Mutating operations take an
//! `Idempotency-Key` header: a repeated request with the same key returns the first reply and changes nothing.

mod config;
mod events;
mod gates;
mod mcp;
mod runs;
mod triggers;

use aae_core::{
    CoreHandle, Journal,
    config::Config,
    run::{Msg, Reader},
};
use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{Arc, PoisonError, RwLock};
use tokio::{net::UnixListener, sync::watch};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

/// What the handlers share.
pub struct Api {
    pub core: CoreHandle<Msg>,
    pub reader: Reader,
    /// A reading instance of the daemon's journal.
    pub journal: Journal,
    /// The last `seq` the daemon's journal wrote (`Journal::subscribe`).
    pub written: watch::Receiver<i64>,
    /// The daemon's own facts for `GET /v1/daemon`.
    pub daemon: Value,
    /// The config the daemon runs with; a reload replaces it.
    pub config: RwLock<Arc<Config>>,
    /// The generation `config` is (1 at start); held from reading the file to replacing `config`, so reloads apply
    /// in order and a replayed reload does not replace it.
    pub applied: tokio::sync::Mutex<u64>,
    /// Reads the daemon's config file with every check a start makes, or says what is wrong in it.
    pub load: Box<dyn Fn() -> Result<Config, Vec<String>> + Send + Sync>,
}

impl Api {
    /// The config the daemon runs with now.
    fn config(&self) -> Arc<Config> {
        self.config.read().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

type State = axum::extract::State<Arc<Api>>;

#[derive(OpenApi)]
#[openapi(info(title = "aae", description = "The API of the aae daemon (spec §12)."))]
struct Doc;

fn router() -> OpenApiRouter<Arc<Api>> {
    OpenApiRouter::with_openapi(Doc::openapi())
        .routes(routes!(runs::create, runs::list))
        .routes(routes!(runs::get))
        .routes(routes!(runs::cancel))
        .routes(routes!(runs::answer))
        .routes(routes!(runs::finish))
        .routes(routes!(gates::list, gates::close))
        .routes(routes!(gates::open))
        .routes(routes!(triggers::fire))
        .routes(routes!(triggers::list))
        .routes(routes!(config::list_projects))
        .routes(routes!(config::list_agents))
        .routes(routes!(config::list_formulas))
        .routes(routes!(config::list_packs))
        .routes(routes!(config::list_plugins))
        .routes(routes!(config::explain))
        .routes(routes!(config::check))
        .routes(routes!(config::reload))
        .routes(routes!(events::stream))
        .routes(routes!(daemon))
}

/// The OpenAPI document of the API, generated from the handlers.
pub fn openapi() -> utoipa::openapi::OpenApi {
    router().split_for_parts().1
}

/// Serves the API on `listener` until `stop` resolves; open requests and event streams are cut then (a request
/// cut after its commit is repeated safely with its idempotency key).
pub async fn serve(listener: UnixListener, api: Api, stop: impl Future<Output = ()>) -> std::io::Result<()> {
    // MCP is not a REST operation: it stays out of the OpenAPI document.
    let app = router()
        .split_for_parts()
        .0
        .route("/mcp", axum::routing::post(mcp::post).get(mcp::get))
        .with_state(Arc::new(api));
    tokio::select! {
        served = axum::serve(listener, app) => served,
        () = stop => Ok(()),
    }
}

/// The daemon: its version, process and socket.
#[utoipa::path(get, path = "/v1/daemon", responses((status = OK, body = Object)))]
async fn daemon(state: State) -> Json<Value> {
    let mut daemon = state.daemon.clone();
    daemon["journal_seq"] = json!(*state.written.borrow());
    Json(daemon)
}

/// An error reply: `{"error": {"code", "message"}}`.
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    /// More fields of the error object.
    extra: Value,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            extra: Value::Null,
        }
    }
}

impl From<aae_core::Error> for ApiError {
    fn from(e: aae_core::Error) -> Self {
        let (status, code) = match &e {
            aae_core::Error::Rejected(_) => (StatusCode::CONFLICT, "rejected"),
            aae_core::Error::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage"),
            aae_core::Error::Stopped => (StatusCode::SERVICE_UNAVAILABLE, "stopped"),
        };
        Self::new(status, code, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let (Value::Object(extra), Some(error)) = (self.extra, error.as_object_mut()) {
            error.extend(extra);
        }
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}

/// A JSON body whose rejection is an `ApiError` like every other error.
pub(crate) struct Body<T>(T);

impl<T: serde::de::DeserializeOwned, S: Send + Sync> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        let json: Result<Json<T>, JsonRejection> = Json::from_request(req, state).await;
        json.map(|Json(t)| Self(t))
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))
    }
}

/// The `Idempotency-Key` of a request.
fn key(headers: &HeaderMap) -> Option<&str> {
    headers.get("idempotency-key").and_then(|v| v.to_str().ok())
}

type Reply = Result<Json<Value>, ApiError>;
