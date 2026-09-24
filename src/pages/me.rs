use axum::extract::{Path, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HOST};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::error_page::{error_page, OauthError, AUTH_STYLE};
use crate::session::owned_calendar_or_error;
use crate::AppState;

pub async fn delete_calendar(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    if let Err(e) = sqlx::query("DELETE FROM calendars WHERE id = $1")
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to delete calendar {}: {}", cal.id, e);
        return error_page(OauthError::FailedToDeleteCalendar);
    }

    Redirect::to("/me").into_response()
}

fn build_caldav_mobileconfig(host: &str, username: &str, password: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>PayloadContent</key>
    <array>
        <dict>
            <key>CalDAVAccountDescription</key>
            <string>NotionCal</string>
            <key>CalDAVHostName</key>
            <string>{host}</string>
            <key>CalDAVPassword</key>
            <string>{password}</string>
            <key>CalDAVPort</key>
            <integer>443</integer>
            <key>CalDAVUseSSL</key>
            <true/>
            <key>CalDAVUsername</key>
            <string>{username}</string>
            <key>PayloadDescription</key>
            <string>Adds a two-way CalDAV account for NotionCal.</string>
            <key>PayloadDisplayName</key>
            <string>NotionCal CalDAV Account</string>
            <key>PayloadIdentifier</key>
            <string>vn.opendiy.notion-caldav.caldav.{username}</string>
            <key>PayloadType</key>
            <string>com.apple.caldav.account</string>
            <key>PayloadUUID</key>
            <string>{account_uuid}</string>
            <key>PayloadVersion</key>
            <integer>1</integer>
        </dict>
    </array>
    <key>PayloadDisplayName</key>
    <string>NotionCal</string>
    <key>PayloadIdentifier</key>
    <string>vn.opendiy.notion-caldav.profile.{username}</string>
    <key>PayloadType</key>
    <string>Configuration</string>
    <key>PayloadUUID</key>
    <string>{profile_uuid}</string>
    <key>PayloadVersion</key>
    <integer>1</integer>
</dict>
</plist>
"#,
        account_uuid = uuid::Uuid::new_v4(),
        profile_uuid = uuid::Uuid::new_v4(),
    )
}

#[derive(Deserialize)]
pub struct MobileconfigQuery {
    token: String,
}

fn invalid_mobileconfig_link_page() -> axum::response::Response {
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="logout" href="/">Home</a></div>
<p class="hint">This download link is invalid or has expired — the password may have been regenerated since. Log in and click "Regenerate" to get a new one.</p>
</body></html>"#
    ))
    .into_response()
}

pub async fn download_account_mobileconfig(
    State(state): State<AppState>,
    Query(query): Query<MobileconfigQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT account_caldav_username, account_caldav_password FROM users WHERE mobileconfig_token = $1",
    )
    .bind(&query.token)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((username, password)) = row else {
        return invalid_mobileconfig_link_page();
    };

    let plist = build_caldav_mobileconfig(&host, &username, &password);
    (
        [
            (CONTENT_TYPE, "application/x-apple-aspen-config"),
            (
                CONTENT_DISPOSITION,
                "attachment; filename=\"notioncal.mobileconfig\"",
            ),
        ],
        plist,
    )
        .into_response()
}
