use axum::{body::Bytes, http::HeaderMap};
use tracing::info;

/// Temporary test endpoint for verifying the Notion webhook handshake.
/// Notion POSTs `{"verification_token": "..."}` once on subscription
/// creation, then real events signed via `X-Notion-Signature` afterward.
/// This just logs everything so the token/payload can be read from
/// `kubectl logs` during setup.
pub async fn handle_notion_webhook_test(headers: HeaderMap, body: Bytes) -> &'static str {
    let body_str = String::from_utf8_lossy(&body);
    let signature = headers
        .get("x-notion-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    info!(signature, body = %body_str, "notion webhook test payload received");
    "ok"
}
