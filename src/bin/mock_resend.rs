//! A stand-in Resend API for staging's load/regression tests, the same
//! reason mock_notion exists: so a test run never sends real email through
//! the real account (spam risk to whoever's inbox a test used, and Resend's
//! own quota), and doesn't need a real RESEND_API_KEY -- any bearer token is
//! accepted.
//!
//! Captures what was sent in memory (lost on restart, which is fine --
//! nothing here is meant to survive one) and exposes it at GET /sent so a
//! test can assert on subject/to/html instead of just "did the app not
//! crash".

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, serde::Serialize)]
struct SentEmail {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
}

type SharedState = Arc<Mutex<Vec<SentEmail>>>;

async fn send(State(state): State<SharedState>, headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
    if !headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"message": "missing bearer token"})),
        );
    }

    let email = SentEmail {
        from: body
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        to: body
            .get("to")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        subject: body
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        html: body
            .get("html")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    };
    tracing::info!(to = ?email.to, subject = %email.subject, "mock-resend: captured email");
    state.lock().await.push(email);

    (
        StatusCode::OK,
        Json(json!({"id": format!("mock-email-{}", uuid_like())})),
    )
}

async fn sent(State(state): State<SharedState>) -> impl IntoResponse {
    Json(state.lock().await.clone())
}

async fn status_page(State(state): State<SharedState>) -> Html<String> {
    let count = state.lock().await.len();
    Html(format!(
        "<h1>mock-resend</h1><p>{count} email(s) captured since start.</p><p>See <a href=\"/sent\">/sent</a> for the raw list.</p>"
    ))
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let port: u16 = std::env::var("MOCK_RESEND_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3002);

    let state: SharedState = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new()
        .route("/", get(status_page))
        .route("/status", get(status_page))
        .route("/sent", get(sent))
        .route("/emails", post(send))
        .with_state(state);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    tracing::info!("mock-resend listening on {addr} — status page at http://localhost:{port}/status");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
