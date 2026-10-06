use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::crypto::generate_token;
use crate::error_page::{error_page, OauthError, AUTH_STYLE};
use crate::session::find_or_create_user;
use crate::AppState;

pub(crate) const NOTION_VERSION: &str = "2025-09-03";

#[derive(Debug, Clone)]
pub struct NotionOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl NotionOAuthConfig {
    pub fn from_env(app_base_url: &str) -> Option<Self> {
        let client_id = std::env::var("NOTION_OAUTH_CLIENT_ID").ok()?;
        let client_secret = std::env::var("NOTION_OAUTH_CLIENT_SECRET").ok()?;
        Some(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{app_base_url}/oauth/notion/callback"),
        })
    }
}

pub async fn connect_notion_start(
    State(state): State<AppState>,
    session: tower_sessions::Session,
) -> impl IntoResponse {
    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(OauthError::NotionNotConfigured);
    };

    let oauth_state = generate_token(32);
    if let Err(e) = session.insert("notion_oauth_state", &oauth_state).await {
        error!("failed to store notion oauth state: {}", e);
        return error_page(OauthError::TryAgain);
    }

    let mut url = url::Url::parse(&format!("{}/v1/oauth/authorize", state.notion_api_base_url))
        .expect("valid notion_api_base_url");
    url.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("owner", "user")
        .append_pair("redirect_uri", &cfg.redirect_uri)
        .append_pair("state", &oauth_state);

    Redirect::to(url.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn notion_oauth_callback(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    if let Some(err) = params.error {
        return error_page(OauthError::NotionDenied(err));
    }
    let Some(code) = params.code else {
        return error_page(OauthError::MissingAuthCode);
    };

    let expected_state: Option<String> = session.get("notion_oauth_state").await.unwrap_or(None);
    let _ = session.remove::<String>("notion_oauth_state").await;
    if expected_state.is_none() || expected_state.as_deref() != params.state.as_deref() {
        return error_page(OauthError::InvalidSession);
    }

    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(OauthError::NotionNotConfigured);
    };

    let resp = match state
        .client
        .post(format!("{}/v1/oauth/token", state.notion_api_base_url))
        .basic_auth(&cfg.client_id, Some(&cfg.client_secret))
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": cfg.redirect_uri,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("notion token exchange request failed: {}", e);
            return error_page(OauthError::CantReachNotion);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        error!("notion token exchange failed {}: {}", status, txt);
        return error_page(OauthError::NotionRejectedToken);
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to parse notion token response: {}", e);
            return error_page(OauthError::InvalidNotionResponse);
        }
    };

    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_id = body
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_name = body
        .get("workspace_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let bot_id = body
        .get("bot_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if access_token.is_empty() || workspace_id.is_empty() {
        return error_page(OauthError::NotionResponseMissingFields);
    }

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email).await {
        Ok(id) => id,
        Err(e) => {
            error!("failed to find_or_create_user: {}", e);
            return error_page(OauthError::Generic);
        }
    };

    let connection_id: i64 = match sqlx::query_scalar(
        "INSERT INTO notion_connections (user_id, notion_access_token, workspace_id, workspace_name, bot_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, workspace_id) DO UPDATE SET
             notion_access_token = EXCLUDED.notion_access_token,
             workspace_name = EXCLUDED.workspace_name,
             bot_id = EXCLUDED.bot_id,
             token_invalid_since = NULL
         RETURNING id",
    )
    .bind(user_id)
    .bind(&access_token)
    .bind(&workspace_id)
    .bind(&workspace_name)
    .bind(&bot_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!("failed to upsert notion_connection: {}", e);
            return error_page(OauthError::FailedToSaveConnection);
        }
    };

    Redirect::to(&format!(
        "/connect/notion/databases?connection_id={connection_id}"
    ))
    .into_response()
}

/// No-login quick-action link sent in the token-invalid notification email
/// (see `AppState::mark_notion_connection_invalid_and_notify`). Capability-
/// based on the unguessable `action_token`, same trust model as
/// `users.mobileconfig_token`. Removes the connection's calendars (stops
/// syncing, drops their CalDAV subscriptions) but leaves the
/// `notion_connections` row itself — reconnecting that workspace later
/// still finds and revives it via the `(user_id, workspace_id)` upsert
/// above.
pub async fn stop_sync_by_action_token(
    State(state): State<AppState>,
    Path(action_token): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query(
        "DELETE FROM calendars WHERE notion_connection_id = (
             SELECT id FROM notion_connections WHERE action_token = $1
         )",
    )
    .bind(&action_token)
    .execute(&state.db)
    .await;

    let message = match result {
        Ok(r) if r.rows_affected() > 0 => format!(
            "Syncing stopped for {} calendar{}. You can reconnect anytime from your dashboard.",
            r.rows_affected(),
            if r.rows_affected() == 1 { "" } else { "s" }
        ),
        Ok(_) => {
            "Nothing to stop — this link has already been used or is no longer valid.".to_string()
        }
        Err(e) => {
            error!("failed to stop sync for action_token: {}", e);
            "Something went wrong. Please try again or contact support.".to_string()
        }
    };

    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong></div>
<p class="hint">{}</p>
</body></html>"#,
        crate::session::html_escape(&message)
    ))
}
