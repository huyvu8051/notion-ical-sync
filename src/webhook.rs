use axum::{body::Bytes, extract::State, http::HeaderMap};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{info, warn};

use crate::AppState;

type HmacSha256 = Hmac<Sha256>;

/// Receives Notion's webhook events (see developers.notion.com/reference/webhooks).
///
/// Two request shapes hit this endpoint:
/// - The one-time verification handshake: `{"verification_token": "..."}`,
///   sent unsigned when a subscription is created/re-verified. We just log
///   it (the token is copy-pasted into Notion's UI by hand to activate).
/// - Real events, signed via `X-Notion-Signature: sha256=<hmac-hex>` using
///   that same verification_token as the HMAC key. On a valid signature we
///   trigger an immediate refresh of the affected database instead of
///   waiting for the next poll cycle.
pub async fn handle_notion_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> &'static str {
    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "notion webhook: invalid JSON body");
            return "ignored";
        }
    };

    if let Some(token) = json.get("verification_token").and_then(|v| v.as_str()) {
        info!(
            verification_token = token,
            "notion webhook verification handshake received"
        );
        return "ok";
    }

    let Some(secret) = state.webhook_secret.as_deref() else {
        warn!("notion webhook: NOTION_WEBHOOK_SECRET not configured, dropping unverifiable event");
        return "ignored";
    };

    let signature = headers
        .get("x-notion-signature")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("sha256="));

    let Some(signature) = signature else {
        warn!("notion webhook: missing X-Notion-Signature header");
        return "ignored";
    };

    let Ok(expected) = hex_decode(signature) else {
        warn!("notion webhook: malformed signature header");
        return "ignored";
    };

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(&body);
    if mac.verify_slice(&expected).is_err() {
        warn!("notion webhook: signature verification failed, dropping event");
        return "ignored";
    }

    let event_type = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let data_source_id = json
        .pointer("/data/parent/data_source_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // `data.id` is the affected page's id for page.* events — absent for
    // database/data_source-level events, which is fine, sync_log just shows
    // an empty event_uid for those.
    let page_id = json
        .pointer("/data/id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    info!(event_type = %event_type, data_source_id = data_source_id.as_deref(), "notion webhook event verified");

    if let Some(ds_id) = data_source_id {
        tokio::spawn(async move {
            // Logged here (source="notion") rather than inside
            // refresh_by_data_source, since that function is also called by
            // the plain 10-minute poll and by CalDAV/webview writes'
            // post-write refresh — this is specifically the "Notion pushed
            // us a change" case a user watching their sync log cares about.
            if let Some(cal) = state.calendar_by_data_source_id(&ds_id).await {
                state
                    .log_sync(cal.id, "notion", &event_type, &page_id, &page_id, "ok", "")
                    .await;
            }
            state.refresh_by_data_source(&ds_id).await;
        });
    }

    "ok"
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if !s.len().is_multiple_of(2) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}
