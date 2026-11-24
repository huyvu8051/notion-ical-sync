use axum::{body::Bytes, extract::State, http::HeaderMap};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{info, warn};

use crate::AppState;

type HmacSha256 = Hmac<Sha256>;

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

    if let Some(verification_token) = json.get("verification_token").and_then(|v| v.as_str()) {
        info!(
            verification_token,
            "notion webhook verification handshake received"
        );
        return "ok";
    }

    let Some(webhook_secret) = state.webhook_secret.as_deref() else {
        warn!("notion webhook: NOTION_WEBHOOK_SECRET not configured, dropping unverifiable event");
        return "ignored";
    };

    let signature_header = headers
        .get("x-notion-signature")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("sha256="));

    let Some(signature_hex) = signature_header else {
        warn!("notion webhook: missing X-Notion-Signature header");
        return "ignored";
    };

    let Ok(expected_signature) = hex_decode(signature_hex) else {
        warn!("notion webhook: malformed signature header");
        return "ignored";
    };

    let mut mac =
        HmacSha256::new_from_slice(webhook_secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(&body);
    if mac.verify_slice(&expected_signature).is_err() {
        warn!("notion webhook: signature verification failed, dropping event");
        return "ignored";
    }

    // The payload shape itself, captured verbatim. Nothing else in this
    // service records it: the four fields pulled out below are all that ever
    // reached a log line, and `body` is dropped at the end of this function,
    // so there has never been a way to answer "what does a
    // `data_source.schema_updated` event actually look like" without waiting
    // for one to arrive and reading it live. Off by default (it is one log
    // line per webhook, ~1400/day, and it duplicates what the fields below
    // already say); set NOTION_WEBHOOK_LOG_RAW_BODY=true for as long as it
    // takes to collect the shapes you need, then unset it.
    //
    // Deliberately after signature verification: an unverified body is
    // attacker-controlled and does not belong in the log store.
    if std::env::var("NOTION_WEBHOOK_LOG_RAW_BODY").as_deref() == Ok("true") {
        info!(
            raw_body = %String::from_utf8_lossy(&body),
            "notion webhook raw body"
        );
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
    // Which page the event is about, and what kind of thing it is. Measured
    // across 112 production webhooks: `entity.id` was present in every one,
    // and `entity.type` was "page" in all but two `view.created` events.
    let entity_id = json
        .pointer("/entity/id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let entity_type = json
        .pointer("/entity/type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    info!(
        event_type = %event_type,
        data_source_id = data_source_id.as_deref(),
        entity_id = entity_id.as_deref(),
        entity_type = %entity_type,
        "notion webhook event verified"
    );

    if let Some(data_source_id) = data_source_id {
        tokio::spawn(async move {
            // A page event names its page, so read that one row rather than
            // re-querying every row in the data source. Anything else — a
            // view or schema change — says nothing about which rows moved, so
            // it still needs the full re-query.
            match entity_id.filter(|_| entity_type == "page") {
                Some(page_id) => {
                    state
                        .refresh_page_by_data_source(&data_source_id, &page_id)
                        .await
                }
                None => state.refresh_by_data_source(&data_source_id).await,
            }
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
