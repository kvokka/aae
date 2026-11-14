//! The HTTP API (spec §12): JSON over a unix socket, the event stream as SSE. Mutating operations take an
//! `Idempotency-Key` header: a repeated request with the same key returns the first reply and changes nothing; the
//! key repeated with another method, path or body is refused with 422.

pub(crate) mod config;
mod daemon;
mod events;
mod gates;
mod instances;
mod live;
pub mod mcp;
mod replies;
mod runs;
mod triggers;

use aae_core::{
    CoreHandle, Journal,
    config::Config,
    driver::Driver,
    run::{Msg, Reader},
};
use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value};
use std::sync::{Arc, PoisonError, RwLock};
use tokio::{
    net::{TcpListener, UnixListener},
    sync::watch,
};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

/// What the handlers share.
pub struct Api {
    pub core: CoreHandle<Msg>,
    pub reader: Reader,
    /// A reading instance of the daemon's journal.
    pub journal: Journal,
    /// The last `seq` the daemon's journal wrote (`Journal::subscribe`).
    pub written: watch::Receiver<i64>,
    /// How the journal writes go (`Journal::health`).
    pub journal_health: watch::Receiver<aae_core::JournalHealth>,
    /// The daemon's own facts for `GET /v1/daemon`.
    pub daemon: Value,
    /// Why the daemon stops (`drain` or `restart`) once a drain is over; `serve` returns then.
    pub stop: watch::Sender<Option<&'static str>>,
    /// The config the daemon runs with; a reload replaces it.
    pub config: RwLock<Arc<Config>>,
    /// The generation `config` is (1 at start); held from reading the file to replacing `config`, so reloads apply
    /// in order and a replayed reload does not replace it.
    pub applied: tokio::sync::Mutex<u64>,
    /// Reads the daemon's config file with every check a start makes, or says what is wrong in it.
    pub load: Box<dyn Fn() -> Result<Config, Vec<String>> + Send + Sync>,
    /// The harness drivers, whose session streams the live view of a run relays.
    pub drivers: Vec<Arc<dyn Driver>>,
    /// The checks of `aae doctor`, made from the daemon's process: one `{check, level, detail}` object each.
    pub doctor: Box<dyn Fn() -> Vec<Value> + Send + Sync>,
}

impl Api {
    /// The config the daemon runs with now.
    fn config(&self) -> Arc<Config> {
        self.config.read().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

type State = axum::extract::State<Arc<Api>>;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "aae",
        description = "The API of the aae daemon (spec §12). Over TCP every operation needs \
            `Authorization: Bearer <api_token>`; without it the reply is 401 `unauthorized`, an `ErrorReply`."
    ),
    components(schemas(ErrorReply))
)]
struct Doc;

fn router() -> OpenApiRouter<Arc<Api>> {
    OpenApiRouter::with_openapi(Doc::openapi())
        .routes(routes!(runs::create, runs::list))
        .routes(routes!(runs::get))
        .routes(routes!(live::live))
        .routes(routes!(runs::cancel))
        .routes(routes!(runs::answer))
        .routes(routes!(runs::finish))
        .routes(routes!(gates::list, gates::close))
        .routes(routes!(gates::open))
        .routes(routes!(triggers::fire))
        .routes(routes!(triggers::enable))
        .routes(routes!(triggers::disable))
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
        .routes(routes!(daemon::status))
        .routes(routes!(daemon::drain))
        .routes(routes!(daemon::restart))
        .routes(routes!(daemon::doctor))
        .routes(routes!(instances::list))
        .routes(routes!(instances::stop))
}

/// The OpenAPI document of the API, generated from the handlers.
pub fn openapi() -> utoipa::openapi::OpenApi {
    router().split_for_parts().1
}

/// Serves the API on `listener`, and on `tcp` with `Authorization: Bearer <api_token>`, until `signal` resolves
/// (the reason is `signal` then) or a drain is over (its reason); open requests and event streams are cut then (a
/// request cut after its commit is repeated safely with its idempotency key).
pub async fn serve(
    listener: UnixListener,
    tcp: Option<TcpListener>,
    api: Api,
    signal: impl Future<Output = ()>,
) -> std::io::Result<&'static str> {
    let mut drained = api.stop.subscribe();
    let bearer: Arc<str> = format!("Bearer {}", api.config().api_token).into();
    // MCP is not a REST operation: it stays out of the OpenAPI document.
    let app = router()
        .split_for_parts()
        .0
        .route("/mcp", axum::routing::post(mcp::post).get(mcp::get))
        .layer(axum::middleware::from_fn(fingerprint))
        .with_state(Arc::new(api));
    let guarded = app
        .clone()
        .layer(axum::middleware::from_fn(move |request: Request, next: Next| {
            let allowed =
                (request.headers().get("authorization")).is_some_and(|v| same(v.as_bytes(), bearer.as_bytes()));
            async move {
                if allowed {
                    next.run(request).await
                } else {
                    ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "a bearer token is required")
                        .into_response()
                }
            }
        }));
    let tcp = async {
        match tcp {
            Some(tcp) => axum::serve(tcp, guarded).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        served = axum::serve(listener, app) => served.map(|()| "signal"),
        served = tcp => served.map(|()| "signal"),
        () = signal => Ok("signal"),
        reason = drained.wait_for(Option::is_some) => Ok(reason.map_or("signal", |r| r.unwrap_or("signal"))),
    }
}

/// Puts what a request with an `Idempotency-Key` was (method, path, body) into its `aae-request` header for [`key`].
async fn fingerprint(request: Request, next: Next) -> Response {
    if !request.headers().contains_key("idempotency-key") {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    // The limit of a JSON body in axum.
    let Ok(body) = axum::body::to_bytes(body, 2 << 20).await else {
        return ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "the body is unreadable or too large",
        )
        .into_response();
    };
    let request = [
        parts.method.as_str().as_bytes(),
        b" ",
        parts.uri.path().as_bytes(),
        b"\n",
        &body,
    ];
    parts.headers.insert("aae-request", request_hash(request.concat()));
    next.run(Request::from_parts(parts, axum::body::Body::from(body))).await
}

/// FNV-1a of a request: a hash stable across builds, unlike the standard library's.
fn request_hash(bytes: impl IntoIterator<Item = u8>) -> HeaderValue {
    let hash = (bytes.into_iter()).fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    HeaderValue::from(hash)
}

/// Compares in a time that does not depend on where the inputs differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// An error reply, as every failed operation answers.
#[derive(serde::Serialize, ToSchema)]
pub(crate) struct ErrorReply {
    error: ErrorObject,
}

/// `code` for programs, `message` for people; some codes add fields: `oldest` with `gap` (410), `errors` with
/// `config_error` (400).
#[derive(serde::Serialize, ToSchema)]
pub(crate) struct ErrorObject {
    code: &'static str,
    message: String,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// An error reply: `{"error": {"code", "message", ...}}`.
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
            aae_core::Error::KeyReused => (StatusCode::UNPROCESSABLE_ENTITY, "idempotency_key_reused"),
            aae_core::Error::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage"),
            aae_core::Error::Stopped => (StatusCode::SERVICE_UNAVAILABLE, "stopped"),
        };
        Self::new(status, code, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let extra = match self.extra {
            Value::Object(extra) => extra,
            _ => Map::new(),
        };
        let error = ErrorObject {
            code: self.code,
            message: self.message,
            extra,
        };
        (self.status, Json(ErrorReply { error })).into_response()
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

/// A path or query whose rejection is an `ApiError` like every other error.
pub(crate) struct Param<E>(E);

impl<E, S> FromRequestParts<S> for Param<E>
where
    E: FromRequestParts<S, Rejection: std::fmt::Display>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        (E::from_request_parts(parts, state).await)
            .map(Self)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.to_string()))
    }
}

/// The `Idempotency-Key` of a request and what the request was ([`fingerprint`]).
fn key(headers: &HeaderMap) -> Option<(&str, &str)> {
    let get = |name| headers.get(name).and_then(|v: &HeaderValue| v.to_str().ok());
    Some((get("idempotency-key")?, get("aae-request").unwrap_or_default()))
}

type Reply = Result<Json<Value>, ApiError>;

#[cfg(test)]
mod tests {
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
}
