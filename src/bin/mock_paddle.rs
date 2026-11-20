//! A stand-in for Paddle's server-to-server Billing API, for staging's
//! regression/load tests. Covers only the three endpoints billing.rs
//! actually calls from the backend:
//!   PATCH /subscriptions/{id}                 (defer to trial end)
//!   POST  /customers/{id}/portal-sessions      (cancel-flow entry point)
//!   POST  /subscriptions/{id}/cancel           (cancel)
//!
//! Does NOT cover checkout. start_checkout in billing.rs never calls this
//! API at all -- it renders a page that loads Paddle's own
//! cdn.paddle.com/paddle/v2/paddle.js, which runs entirely client-side and
//! always talks to real Paddle (sandbox or live). Nothing server-side can
//! intercept that; see argocd/manifests/notion-caldav-saas-stg/readme.md
//! for why real Paddle sandbox credentials are still needed for that path.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};

async fn defer_subscription(Path(id): Path<String>, Json(_body): Json<Value>) -> impl IntoResponse {
    tracing::info!(subscription_id = %id, "mock-paddle: deferred to trial end");
    (StatusCode::OK, Json(json!({"data": {"id": id}})))
}

async fn portal_session(Path(id): Path<String>, Json(_body): Json<Value>) -> impl IntoResponse {
    tracing::info!(customer_id = %id, "mock-paddle: created portal session");
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "id": format!("ptlsess_mock_{id}"),
                "urls": {
                    "subscriptions": [{"cancel_subscription": "https://notion-caldav-stg.opendiy.vn/me?mock-cancel-link=1"}],
                    "general": {"overview": "https://notion-caldav-stg.opendiy.vn/me?mock-portal=1"}
                }
            }
        })),
    )
}

async fn cancel_subscription(Path(id): Path<String>, Json(_body): Json<Value>) -> impl IntoResponse {
    tracing::info!(subscription_id = %id, "mock-paddle: canceled");
    (StatusCode::OK, Json(json!({"data": {"id": id, "status": "canceled"}})))
}

async fn status_page() -> Html<&'static str> {
    Html("<h1>mock-paddle</h1><p>Covers portal-session creation, subscription cancel, and defer-to-trial-end only -- not checkout. See the comment at the top of mock_paddle.rs.</p>")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let port: u16 = std::env::var("MOCK_PADDLE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3003);

    let app = Router::new()
        .route("/", get(status_page))
        .route("/status", get(status_page))
        .route("/subscriptions/{id}", patch(defer_subscription))
        .route("/customers/{id}/portal-sessions", post(portal_session))
        .route("/subscriptions/{id}/cancel", post(cancel_subscription));

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    tracing::info!("mock-paddle listening on {addr} — status page at http://localhost:{port}/status");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
