//! Notion OAuth: this app's permission to read/write a specific user's own
//! Notion workspace — a "Public Integration" grant, separate from the SaaS's
//! own login in auth.rs (Keycloak). A user authenticates via Keycloak first,
//! then goes through this flow to grant Notion access and pick which
//! databases become calendars.

use std::collections::HashMap;

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use rand::Rng;
use serde::Deserialize;
use tracing::error;

use crate::auth::{find_or_create_user, html_escape, AUTH_STYLE};
use crate::AppState;

const NOTION_VERSION: &str = "2025-09-03";

/// Config for the Notion Public Integration OAuth flow. Absent (`None`) in
/// AppState until `NOTION_OAUTH_CLIENT_ID`/`NOTION_OAUTH_CLIENT_SECRET` are
/// set — connect routes render a "not configured" page instead of panicking,
/// same posture as `webhook_secret` being optional.
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

fn generate_token(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt)?.to_string())
}

fn error_page(message: &str) -> axum::response::Response {
    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/me">Quay lại</a></div>
<p class="hint">{}</p>
</body></html>"#,
        html_escape(message)
    ))
    .into_response()
}

/// Step 1 confirmation screen (matches the Stitch "Connect Notion" design) —
/// shown before we actually redirect away to Notion's consent screen.
pub async fn connect_notion_page(_claims: OidcClaims<EmptyAdditionalClaims>) -> impl IntoResponse {
    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kết nối Notion — Notion CalDAV SaaS</title>{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/logout">Đăng xuất</a></div>
<div class="connect-card">
  <h1>Kết nối không gian làm việc Notion của bạn</h1>
  <p class="hint">Chúng tôi cần quyền truy cập vào không gian làm việc Notion của bạn để tìm và đồng bộ hóa các cơ sở dữ liệu bạn chọn. Bạn sẽ chọn chính xác trang nào cần chia sẻ ở bước tiếp theo trên Notion.</p>
  <a class="connect-btn" href="/connect/notion/start">Kết nối với Notion</a>
  <ul class="reassure-list">
    <li>Chỉ đọc và ghi vào các trang bạn cho phép</li>
    <li>Có thể ngắt kết nối bất cứ lúc nào</li>
    <li>Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba</li>
  </ul>
  <p class="hint" style="margin-top:1.5rem"><a href="/privacy">Privacy Policy</a> · <a href="/terms">Terms of Service</a></p>
</div>
</body></html>"#
    ))
}

/// Actually redirects to Notion's OAuth consent screen, stashing a CSRF
/// state token in the session first.
pub async fn connect_notion_start(State(state): State<AppState>, session: tower_sessions::Session) -> impl IntoResponse {
    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page("Notion OAuth chưa được cấu hình trên server này.");
    };

    let oauth_state = generate_token(32);
    if let Err(e) = session.insert("notion_oauth_state", &oauth_state).await {
        error!("failed to store notion oauth state: {}", e);
        return error_page("Có lỗi xảy ra, vui lòng thử lại.");
    }

    let mut url = url::Url::parse("https://api.notion.com/v1/oauth/authorize").expect("static url");
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

/// Notion redirects here after the user grants (or denies) access. Exchanges
/// the code for an access token and upserts `notion_connections`.
pub async fn notion_oauth_callback(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    if let Some(err) = params.error {
        return error_page(&format!("Notion từ chối cấp quyền: {err}"));
    }
    let Some(code) = params.code else {
        return error_page("Thiếu mã xác thực từ Notion.");
    };

    let expected_state: Option<String> = session.get("notion_oauth_state").await.unwrap_or(None);
    let _ = session.remove::<String>("notion_oauth_state").await;
    if expected_state.is_none() || expected_state.as_deref() != params.state.as_deref() {
        return error_page("Phiên xác thực không hợp lệ, vui lòng thử lại.");
    }

    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page("Notion OAuth chưa được cấu hình trên server này.");
    };

    let resp = match state
        .client
        .post("https://api.notion.com/v1/oauth/token")
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
            return error_page("Không thể kết nối tới Notion.");
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        error!("notion token exchange failed {}: {}", status, txt);
        return error_page("Notion từ chối yêu cầu trao đổi token.");
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to parse notion token response: {}", e);
            return error_page("Phản hồi từ Notion không hợp lệ.");
        }
    };

    let access_token = body.get("access_token").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let workspace_id = body.get("workspace_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let workspace_name = body.get("workspace_name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let bot_id = body.get("bot_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();

    if access_token.is_empty() || workspace_id.is_empty() {
        return error_page("Phản hồi từ Notion thiếu access_token hoặc workspace_id.");
    }

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(e) => {
            error!("failed to find_or_create_user: {}", e);
            return error_page("Có lỗi xảy ra.");
        }
    };

    let connection_id: i64 = match sqlx::query_scalar(
        "INSERT INTO notion_connections (user_id, notion_access_token, workspace_id, workspace_name, bot_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, workspace_id) DO UPDATE SET
             notion_access_token = EXCLUDED.notion_access_token,
             workspace_name = EXCLUDED.workspace_name,
             bot_id = EXCLUDED.bot_id
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
            return error_page("Không thể lưu kết nối Notion.");
        }
    };

    Redirect::to(&format!("/connect/notion/databases?connection_id={connection_id}")).into_response()
}

struct DatabaseCandidate {
    database_id: String,
    data_source_id: String,
    title: String,
    icon_emoji: Option<String>,
    /// Name of the first date-typed property found, if any. Databases
    /// without one can't be synced (refresh_db needs a date property to
    /// filter/sort on).
    date_property: Option<String>,
}

/// Searches the workspace for every data source (Notion's 2025-09-03 API
/// split "database" into a container plus one-or-more data sources —
/// `properties` now lives on the data source, not the database, so search
/// for `data_source` objects directly rather than `database` objects) the
/// user granted access to, checking each one for a date property (needed
/// for both sync and this picker's compatibility check).
async fn list_syncable_databases(client: &reqwest::Client, token: &str) -> Result<Vec<DatabaseCandidate>, String> {
    let resp = client
        .post("https://api.notion.com/v1/search")
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({ "filter": { "value": "data_source", "property": "object" } }))
        .send()
        .await
        .map_err(|e| format!("search request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Notion search error {status}: {txt}"));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse failed: {e}"))?;
    let results = body.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();

    let mut candidates = Vec::new();
    for ds in results {
        let Some(data_source_id) = ds.get("id").and_then(|v| v.as_str()) else { continue };
        let Some(database_id) = ds
            .get("parent")
            .and_then(|p| p.get("database_id"))
            .and_then(|id| id.as_str())
        else {
            continue; // parent isn't a database (shouldn't happen for object=data_source, but be defensive)
        };

        let title = ds
            .get("title")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("plain_text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)")
            .to_string();
        let icon_emoji = ds
            .get("icon")
            .and_then(|icon| icon.get("emoji"))
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let date_property = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .and_then(|props| {
                props
                    .iter()
                    .find(|(_, def)| def.get("type").and_then(|t| t.as_str()) == Some("date"))
                    .map(|(name, _)| name.clone())
            });

        candidates.push(DatabaseCandidate {
            database_id: database_id.to_string(),
            data_source_id: data_source_id.to_string(),
            title,
            icon_emoji,
            date_property,
        });
    }

    Ok(candidates)
}

async fn connection_token_for_user(state: &AppState, connection_id: i64, user_id: i64) -> Option<String> {
    sqlx::query_scalar(
        "SELECT notion_access_token FROM notion_connections WHERE id = $1 AND user_id = $2",
    )
    .bind(connection_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None)
}

#[derive(Debug, Deserialize)]
pub struct DatabasesPageParams {
    connection_id: i64,
}

/// Step 2 (matches the Stitch "Pick a database" design) — lists every
/// database found in the just-connected workspace, disabling ones without a
/// date property.
pub async fn pick_databases_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Query(params): Query<DatabasesPageParams>,
) -> impl IntoResponse {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(_) => return error_page("Có lỗi xảy ra."),
    };

    let Some(access_token) = connection_token_for_user(&state, params.connection_id, user_id).await else {
        return error_page("Không tìm thấy kết nối Notion này.");
    };

    let candidates = match list_syncable_databases(&state.client, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page("Không thể lấy danh sách cơ sở dữ liệu từ Notion.");
        }
    };

    if candidates.is_empty() {
        return Html(format!(
            r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/logout">Đăng xuất</a></div>
<h1>Chọn cơ sở dữ liệu để đồng bộ</h1>
<p class="hint">Không tìm thấy cơ sở dữ liệu nào bạn đã cấp quyền. <a href="/connect/notion/start">Cấp thêm quyền truy cập trên Notion</a>.</p>
</body></html>"#
        ))
        .into_response();
    }

    let rows: String = candidates
        .iter()
        .map(|c| {
            let icon = c.icon_emoji.clone().unwrap_or_else(|| "📄".to_string());
            match &c.date_property {
                Some(date_prop) => format!(
                    r#"<label class="db-card"><input type="checkbox" name="db_ids" value="{db_id}" checked> <span class="db-icon">{icon}</span> <span class="db-name">{title}</span><span class="db-meta">Có thuộc tính ngày: {date_prop}</span></label>"#,
                    db_id = html_escape(&c.database_id),
                    icon = html_escape(&icon),
                    title = html_escape(&c.title),
                    date_prop = html_escape(date_prop),
                ),
                None => format!(
                    r#"<div class="db-card db-card-disabled"><input type="checkbox" disabled> <span class="db-icon">{icon}</span> <span class="db-name">{title}</span><span class="db-warning">Không tìm thấy thuộc tính ngày</span></div>"#,
                    icon = html_escape(&icon),
                    title = html_escape(&c.title),
                ),
            }
        })
        .collect();

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Chọn cơ sở dữ liệu — Notion CalDAV SaaS</title>{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/logout">Đăng xuất</a></div>
<h1>Chọn cơ sở dữ liệu để đồng bộ</h1>
<p class="hint">Chúng tôi đã tìm thấy các cơ sở dữ liệu sau trong không gian làm việc Notion của bạn. Chọn (các) cơ sở dữ liệu bạn muốn biến thành lịch.</p>
<form method="post" action="/connect/notion/databases">
  <input type="hidden" name="connection_id" value="{connection_id}">
  <div class="db-list">{rows}</div>
  <p class="hint"><a href="/connect/notion/start">Không thấy cơ sở dữ liệu bạn cần? Cấp thêm quyền truy cập trên Notion</a></p>
  <div class="action-bar">
    <a class="connect-btn-secondary" href="/me">Quay lại</a>
    <button type="submit" id="continue-btn" class="connect-btn">Tiếp tục</button>
  </div>
</form>
<script>
document.addEventListener('change', function () {{
  var n = document.querySelectorAll('input[name="db_ids"]:checked').length;
  var btn = document.getElementById('continue-btn');
  btn.textContent = n > 0 ? 'Tiếp tục với ' + n + ' cơ sở dữ liệu' : 'Tiếp tục';
}});
</script>
</body></html>"#,
        connection_id = params.connection_id,
    ))
    .into_response()
}

struct CreateCalendarsForm {
    connection_id: i64,
    db_ids: Vec<String>,
}

impl CreateCalendarsForm {
    /// Parsed by hand from the raw body via `url::form_urlencoded` rather
    /// than `axum::Form` — `serde_urlencoded` (what `Form` uses) can't
    /// deserialize a `Vec<String>` field from a single `db_ids=x` pair (only
    /// happens to work when 2+ checkboxes are checked), which broke the
    /// single-database-selected case, the most common one.
    fn parse(body: &str) -> Option<Self> {
        let mut connection_id = None;
        let mut db_ids = Vec::new();
        for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
            match key.as_ref() {
                "connection_id" => connection_id = value.parse::<i64>().ok(),
                "db_ids" => db_ids.push(value.into_owned()),
                _ => {}
            }
        }
        Some(Self { connection_id: connection_id?, db_ids })
    }
}

/// Creates one `calendars` row (with freshly generated CalDAV credentials)
/// per selected, syncable database. Re-fetches the candidate list from
/// Notion server-side rather than trusting metadata from the form, so a
/// tampered request can at most select an id that isn't actually syncable
/// (silently skipped) — never inject arbitrary data_source_id/date_property.
pub async fn create_calendars(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let body = String::from_utf8_lossy(&body);
    let Some(form) = CreateCalendarsForm::parse(&body) else {
        return error_page("Yêu cầu không hợp lệ.");
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(_) => return error_page("Có lỗi xảy ra."),
    };

    let Some(access_token) = connection_token_for_user(&state, form.connection_id, user_id).await else {
        return error_page("Không tìm thấy kết nối Notion này.");
    };

    if form.db_ids.is_empty() {
        return Redirect::to("/me").into_response();
    }

    let candidates = match list_syncable_databases(&state.client, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page("Không thể lấy danh sách cơ sở dữ liệu từ Notion.");
        }
    };

    // (display_name, caldav_username, plaintext password) for calendars
    // actually created just now — shown once on the dashboard, never stored.
    let mut new_credentials: Vec<(String, String, String)> = Vec::new();

    for db_id in &form.db_ids {
        let Some(candidate) = candidates.iter().find(|c| &c.database_id == db_id && c.date_property.is_some()) else {
            continue;
        };
        let date_property = candidate.date_property.clone().expect("checked above");

        let caldav_username = format!("cal_{}", generate_token(12));
        let caldav_password = generate_token(24);
        let password_hash = match hash_password(&caldav_password) {
            Ok(h) => h,
            Err(e) => {
                error!("failed to hash caldav password: {}", e);
                continue;
            }
        };

        let result = sqlx::query(
            "INSERT INTO calendars (user_id, notion_connection_id, database_id, data_source_id, date_property, display_name, caldav_username, caldav_password_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (database_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(form.connection_id)
        .bind(&candidate.database_id)
        .bind(&candidate.data_source_id)
        .bind(&date_property)
        .bind(&candidate.title)
        .bind(&caldav_username)
        .bind(&password_hash)
        .execute(&state.db)
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => new_credentials.push((candidate.title.clone(), caldav_username, caldav_password)),
            Ok(_) => {} // database_id already synced by someone; nothing new to show
            Err(e) => error!("failed to insert calendar {}: {}", candidate.database_id, e),
        }
    }

    if !new_credentials.is_empty() {
        if let Err(e) = session.insert("new_calendar_credentials", &new_credentials).await {
            error!("failed to stash new calendar credentials in session: {}", e);
        }
        state.refresh_all().await;
    }

    Redirect::to("/me").into_response()
}

/// Reads and clears the one-time post-onboarding credential stash written by
/// `create_calendars`, keyed by caldav_username for `me()` to display.
pub async fn take_new_calendar_credentials(session: &tower_sessions::Session) -> HashMap<String, String> {
    let stashed: Vec<(String, String, String)> = session.get("new_calendar_credentials").await.ok().flatten().unwrap_or_default();
    if !stashed.is_empty() {
        let _ = session.remove::<Vec<(String, String, String)>>("new_calendar_credentials").await;
    }
    stashed.into_iter().map(|(_, username, password)| (username, password)).collect()
}
