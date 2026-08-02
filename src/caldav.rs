use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use argon2::password_hash::PasswordVerifier;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{error, info};

// Page info for ICS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: Option<String>,
    pub url: String,
    pub last_edited: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaldavAllowWrites {
    False,
    True,
    Inbox,
}

impl Default for CaldavAllowWrites {
    fn default() -> Self {
        Self::False
    }
}

impl CaldavAllowWrites {
    pub fn from_env() -> Self {
        match std::env::var("CALDAV_ALLOW_WRITES")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "true" => Self::True,
            "inbox" => Self::Inbox,
            _ => Self::False,
        }
    }
}

// Shared app state
#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub db: PgPool,
    pub cache: Arc<RwLock<HashMap<String, Vec<PageInfo>>>>,
    pub caldav_allow_writes: CaldavAllowWrites,
    /// The `verification_token` Notion issued for the webhook subscription,
    /// reused as the HMAC key to authenticate `X-Notion-Signature` on every
    /// subsequent event. None disables signature checking (events are
    /// still logged but not applied) until it's configured.
    pub webhook_secret: Option<String>,
    /// Notion Public Integration OAuth credentials (see oauth.rs). None
    /// disables the "Connect Notion" flow — /connect/notion renders a
    /// "not configured" page instead of panicking, same posture as
    /// `webhook_secret`.
    pub notion_oauth: Option<crate::oauth::NotionOAuthConfig>,
    /// AES-256-GCM key (from `CALDAV_PASSWORD_ENC_KEY`) used to store CalDAV
    /// passwords in a form the dashboard's "Hiện mật khẩu" action can
    /// decrypt later — separate from `caldav_password_hash`, which is what
    /// actual CalDAV Basic Auth verifies against and stays one-way. None
    /// disables reveal (rows just show "Tạo lại" instead), same posture as
    /// `webhook_secret`/`notion_oauth`.
    pub password_enc_key: Option<[u8; 32]>,
}

// Notion API response types
#[derive(Debug, Deserialize)]
struct NotionQueryResponse {
    results: Vec<serde_json::Value>,
}

/// A tracked calendar joined with its owning Notion connection's access
/// token — everything a request needs to talk to Notion on that user's
/// behalf. Looked up per-request from Postgres rather than held as flat
/// AppState fields, since (post multi-tenancy) each row can belong to a
/// different user with a different token.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CalendarRow {
    pub id: i64,
    pub user_id: i64,
    /// The real Notion database id — shared cache key and what Notion API
    /// calls use. Not unique across rows anymore (see migrations/0003):
    /// several users can each have their own subscription to the same
    /// underlying Notion database.
    pub database_id: String,
    /// Per-subscription identifier used in public URLs (/cal/{id},
    /// /app/{id}) and CalDAV auth ownership checks — this, not
    /// `database_id`, is what makes one subscription unambiguous from
    /// another when several users share a `database_id`.
    pub public_id: String,
    pub data_source_id: String,
    pub date_property: String,
    pub display_name: String,
    pub caldav_username: String,
    pub notion_access_token: String,
}

/// Identity resolved from a valid CalDAV Basic Auth credential — attached to
/// the request by `auth_middleware` so handlers that need to scope by owner
/// (e.g. `/refresh`, `/cal.ics`, `/calendars/{user}`) don't have to re-parse
/// and re-verify the Authorization header themselves.
#[derive(Debug, Clone)]
pub struct AuthenticatedCaldavUser {
    pub user_id: i64,
    pub username: String,
}

impl AppState {
    pub fn new(
        db: PgPool,
        caldav_allow_writes: CaldavAllowWrites,
        webhook_secret: Option<String>,
        notion_oauth: Option<crate::oauth::NotionOAuthConfig>,
        password_enc_key: Option<[u8; 32]>,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            db,
            cache: Arc::new(RwLock::new(HashMap::new())),
            caldav_allow_writes,
            webhook_secret,
            notion_oauth,
            password_enc_key,
        }
    }

    pub async fn all_calendars(&self) -> Vec<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, c.date_property, c.display_name, c.caldav_username, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id",
        )
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to list calendars from db: {}", e);
            Vec::new()
        })
    }

    pub async fn calendars_for_user(&self, user_id: i64) -> Vec<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, c.date_property, c.display_name, c.caldav_username, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to list calendars for user {}: {}", user_id, e);
            Vec::new()
        })
    }

    /// Looks up a calendar by its public, URL-facing identifier — the only
    /// lookup that's safe to drive routing/ownership checks off, since
    /// `database_id` alone can now match several different users' rows.
    pub async fn calendar_by_public_id(&self, public_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, c.date_property, c.display_name, c.caldav_username, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.public_id = $1",
        )
        .bind(public_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar {}: {}", public_id, e);
            None
        })
    }

    /// Looks up *a* row for a given Notion database_id — used only by the
    /// legacy host-based aliases (calendar.opendiy.vn/mytime.opendiy.vn, see
    /// `get_public_id_for_host`), a single-owner shortcut that predates
    /// multi-tenancy. Deterministic (oldest row) since database_id is no
    /// longer unique, but still only meaningful for those two hardcoded
    /// hosts — never used for general routing/ownership decisions.
    async fn calendar_by_db_id(&self, db_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, c.date_property, c.display_name, c.caldav_username, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.database_id = $1
             ORDER BY c.created_at ASC
             LIMIT 1",
        )
        .bind(db_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar {}: {}", db_id, e);
            None
        })
    }

    pub async fn calendar_by_data_source_id(&self, ds_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, c.date_property, c.display_name, c.caldav_username, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.data_source_id = $1",
        )
        .bind(ds_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar by data source {}: {}", ds_id, e);
            None
        })
    }

    /// Verifies a CalDAV Basic Auth credential against the `calendars` table,
    /// returning the owning user's id and that calendar's public_id on
    /// success. Callers still need to check the returned public_id against
    /// whatever calendar the request is actually targeting — a valid
    /// credential only proves identity, not that it's for *this* calendar.
    pub async fn verify_caldav_credentials(&self, username: &str, password: &str) -> Option<(i64, String)> {
        #[derive(sqlx::FromRow)]
        struct Row {
            user_id: i64,
            public_id: String,
            caldav_password_hash: String,
        }

        let row: Row = sqlx::query_as::<_, Row>(
            "SELECT user_id, public_id, caldav_password_hash FROM calendars WHERE caldav_username = $1",
        )
        .bind(username)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("caldav credential lookup failed: {}", e);
            None
        })?;

        let hash = argon2::PasswordHash::new(&row.caldav_password_hash).ok()?;
        argon2::Argon2::default().verify_password(password.as_bytes(), &hash).ok()?;
        Some((row.user_id, row.public_id))
    }

    /// Same as `refresh_all` but scoped to one user's own calendars — used by
    /// the per-user `/refresh` CalDAV endpoint so one tenant can't trigger a
    /// refresh (and Notion API calls) for calendars they don't own.
    pub async fn refresh_for_user(&self, user_id: i64) {
        for cal in self.calendars_for_user(user_id).await {
            match self.refresh_db(&cal.data_source_id, &cal.date_property, &cal.notion_access_token).await {
                Ok(pages) => {
                    info!("DB {} synced: {} events", cal.database_id, pages.len());
                    self.cache.write().await.insert(cal.database_id, pages);
                }
                Err(e) => error!("DB {} refresh failed: {}", cal.database_id, e),
            }
        }
    }

    pub async fn refresh_db(&self, ds_id: &str, date_property: &str, notion_token: &str) -> Result<Vec<PageInfo>, String> {
        let url = format!("https://api.notion.com/v1/data_sources/{}/query", ds_id);

        let body = serde_json::json!({
            "filter": {
                "property": date_property,
                "date": { "is_not_empty": true }
            },
            "sorts": [
                { "property": date_property, "direction": "descending" }
            ]
        });

        info!(notion_method = "POST", notion_url = %url, "-> Notion API request");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!(notion_url = %url, error = %e, "<- Notion API request failed (transport)");
                format!("Request failed: {}", e)
            })?;

        let resp_status = resp.status();
        if !resp_status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            error!(notion_url = %url, status = %resp_status, body = %txt, "<- Notion API error response");
            return Err(format!("Notion error {}: {}", resp_status, txt));
        }

        let data: NotionQueryResponse = resp
            .json()
            .await
            .map_err(|e| format!("Parse failed: {}", e))?;
        info!(notion_url = %url, status = %resp_status, page_count = data.results.len(), "<- Notion API response");

        let mut events = Vec::new();
        for page in data.results {
            let props = match page.get("properties") {
                Some(p) => p,
                None => continue,
            };

            let title = props
                .as_object()
                .and_then(|o| o.get("Name"))
                .or_else(|| {
                    props.as_object().and_then(|o| o.values().find(|v| v.get("type").and_then(|t| t.as_str()) == Some("title")))
                })
                .and_then(|t| t.get("title"))
                .and_then(|arr| arr.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("plain_text"))
                .and_then(|t| t.as_str())
                .unwrap_or("(untitled)")
                .to_string();

            let date_val = match props.get(date_property).and_then(|v| v.get("date")) {
                Some(d) => d,
                None => continue,
            };

            let start = date_val
                .get("start")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let end = date_val
                .get("end")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let id = page.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let last_edited = page
                .get("last_edited_time")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let notion_url = format!("https://notion.so/{}", id.replace("-", ""));

            events.push(PageInfo {
                id,
                title,
                start,
                end,
                url: notion_url,
                last_edited,
            });
        }

        Ok(events)
    }

    pub async fn refresh_all(&self) {
        let calendars = self.all_calendars().await;
        // Several rows can now share a database_id (multiple subscribers of
        // the same Notion database) — the cache is keyed by database_id, so
        // only fetch each one once per cycle rather than once per subscriber.
        let mut seen = std::collections::HashSet::new();
        let mut cache = self.cache.write().await;
        for cal in calendars {
            if !seen.insert(cal.database_id.clone()) {
                continue;
            }
            match self.refresh_db(&cal.data_source_id, &cal.date_property, &cal.notion_access_token).await {
                Ok(pages) => {
                    info!("DB {} synced: {} events", cal.database_id, pages.len());
                    cache.insert(cal.database_id, pages);
                }
                Err(e) => error!("DB {} refresh failed: {}", cal.database_id, e),
            }
        }
    }

    /// Refresh just the one database matching `data_source_id`, used to react
    /// to a webhook event immediately instead of waiting for the next poll.
    pub async fn refresh_by_data_source(&self, data_source_id: &str) {
        let Some(cal) = self.calendar_by_data_source_id(data_source_id).await else {
            info!(data_source_id, "webhook event for untracked data source, ignoring");
            return;
        };
        match self.refresh_db(&cal.data_source_id, &cal.date_property, &cal.notion_access_token).await {
            Ok(pages) => {
                info!("DB {} synced via webhook: {} events", cal.database_id, pages.len());
                self.cache.write().await.insert(cal.database_id, pages);
            }
            Err(e) => error!("DB {} webhook-triggered refresh failed: {}", cal.database_id, e),
        }
    }

    fn date_property_value(&self, start: &str, end: Option<&str>) -> serde_json::Value {
        let mut date = serde_json::Map::new();
        date.insert("start".into(), serde_json::json!(start));
        if let Some(e) = end {
            date.insert("end".into(), serde_json::json!(e));
        }
        serde_json::Value::Object(date)
    }

    /// Create a new Notion page under `data_source_id` with a title and the
    /// configured date property set, mirroring what the webview's "add
    /// event" flow needs. Notion is always the source of truth: this writes
    /// through to it directly rather than touching our own cache, which the
    /// caller refreshes afterward from the real Notion state.
    pub async fn notion_create_event(
        &self,
        data_source_id: &str,
        date_property: &str,
        notion_token: &str,
        title: &str,
        start: &str,
        end: Option<&str>,
    ) -> Result<String, String> {
        let mut properties = serde_json::Map::new();
        properties.insert(
            "Name".into(),
            serde_json::json!({ "title": [{ "text": { "content": title } }] }),
        );
        properties.insert(
            date_property.to_string(),
            serde_json::json!({ "date": self.date_property_value(start, end) }),
        );

        let body = serde_json::json!({
            "parent": { "type": "data_source_id", "data_source_id": data_source_id },
            "properties": properties,
        });

        info!(notion_method = "POST", notion_url = "https://api.notion.com/v1/pages", title = %title, "-> Notion API request (create event)");
        let resp = self
            .client
            .post("https://api.notion.com/v1/pages")
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "<- Notion API request failed (transport)");
                format!("Request failed: {}", e)
            })?;

        let resp_status = resp.status();
        if !resp_status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            error!(status = %resp_status, body = %txt, "<- Notion API error response (create event)");
            return Err(format!("Notion error {}: {}", resp_status, txt));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| format!("Parse failed: {}", e))?;
        let page_id = data.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
        info!(status = %resp_status, page_id = ?page_id, "<- Notion API response (create event)");
        page_id
            .ok_or_else(|| "Notion response missing page id".to_string())
    }

    /// Patch title and/or the date property on an existing page. Any field
    /// left as `None` is left untouched on the Notion side.
    pub async fn notion_update_event(
        &self,
        page_id: &str,
        date_property: &str,
        notion_token: &str,
        title: Option<&str>,
        start: Option<&str>,
        end: Option<Option<&str>>,
    ) -> Result<(), String> {
        let mut properties = serde_json::Map::new();
        if let Some(t) = title {
            properties.insert(
                "Name".into(),
                serde_json::json!({ "title": [{ "text": { "content": t } }] }),
            );
        }
        if let Some(s) = start {
            properties.insert(
                date_property.to_string(),
                serde_json::json!({ "date": self.date_property_value(s, end.flatten()) }),
            );
        }

        if properties.is_empty() {
            return Ok(());
        }

        let body = serde_json::json!({ "properties": properties });
        self.patch_page(page_id, notion_token, &body).await
    }

    /// Move a page to trash (Notion has no hard-delete via the public API).
    pub async fn notion_delete_event(&self, page_id: &str, notion_token: &str) -> Result<(), String> {
        self.patch_page(page_id, notion_token, &serde_json::json!({ "in_trash": true })).await
    }

    async fn patch_page(&self, page_id: &str, notion_token: &str, body: &serde_json::Value) -> Result<(), String> {
        let url = format!("https://api.notion.com/v1/pages/{}", page_id);
        info!(notion_method = "PATCH", notion_url = %url, body = %body, "-> Notion API request");
        let resp = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                error!(notion_url = %url, error = %e, "<- Notion API request failed (transport)");
                format!("Request failed: {}", e)
            })?;

        let resp_status = resp.status();
        if !resp_status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            error!(notion_url = %url, status = %resp_status, body = %txt, "<- Notion API error response");
            return Err(format!("Notion error {}: {}", resp_status, txt));
        }
        info!(notion_url = %url, status = %resp_status, "<- Notion API response");
        Ok(())
    }

    pub async fn get_calendar_name(&self, db_id: &str, notion_token: &str) -> String {
        let url = format!("https://api.notion.com/v1/databases/{}", db_id);
        info!(notion_method = "GET", notion_url = %url, "-> Notion API request (calendar name)");
        match self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                let status = r.status();
                let name = r.json::<serde_json::Value>().await
                    .ok()
                    .and_then(|v| v.get("title").cloned())
                    .and_then(|t| {
                        let arr = t.as_array()?;
                        let item = arr.first()?;
                        let txt = item.get("plain_text")?;
                        txt.as_str().map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| format!("Notion {}", &db_id[..8]));
                info!(notion_url = %url, status = %status, name = %name, "<- Notion API response");
                name
            }
            Ok(r) => {
                error!(notion_url = %url, status = %r.status(), "<- Notion API error response (calendar name)");
                format!("Notion {}", &db_id[..8])
            }
            Err(e) => {
                error!(notion_url = %url, error = %e, "<- Notion API request failed (transport)");
                format!("Notion {}", &db_id[..8])
            }
        }
    }
}

pub fn ics_dt(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let value = value.replace(['-', ':'], "");
    if value.contains('T') {
        let mut parts = value.split('T');
        let date = parts.next().unwrap_or("");
        let mut time = parts.next().unwrap_or("");
        if time.contains('+') {
            time = time.split('+').next().unwrap_or(time);
        }
        if time.contains('Z') {
            time = time.split('Z').next().unwrap_or(time);
        }
        if let Some(dot) = time.find('.') {
            time = &time[..dot];
        }
        format!("{}T{}Z", date, time)
    } else {
        format!(";VALUE=DATE:{}", value)
    }
}

pub fn build_ics(db_id: &str, name: &str, pages: &[PageInfo]) -> String {
    let dtstamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let mut ics = String::new();
    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//notion-ical-sync//EN\r\n");
    ics.push_str("CALSCALE:GREGORIAN\r\n");
    ics.push_str("METHOD:PUBLISH\r\n");
    ics.push_str(&format!("X-WR-CALNAME:{}\r\n", name));
    ics.push_str("X-PUBLISHED-TTL:PT1H\r\n");

    for page in pages {
        let start = ics_dt(&page.start);
        let end = page.end.as_deref().map_or(String::new(), ics_dt);

        if start.is_empty() {
            continue;
        }

        ics.push_str("BEGIN:VEVENT\r\n");
        ics.push_str(&format!("UID:{}-{}\r\n", db_id, page.id.replace("-", "")));
        ics.push_str(&format!("DTSTAMP:{}\r\n", dtstamp));
        if start.starts_with(';') {
            ics.push_str(&format!("DTSTART{}\r\n", start));
            if !end.is_empty() {
                ics.push_str(&format!("DTEND{}\r\n", end));
            }
        } else {
            ics.push_str(&format!("DTSTART:{}\r\n", start));
            if !end.is_empty() {
                ics.push_str(&format!("DTEND:{}\r\n", end));
            }
        }
        ics.push_str(&format!("SUMMARY:{}\r\n", escape_ics(&page.title)));
        ics.push_str(&format!("DESCRIPTION:{}\r\n", escape_ics(&page.url)));
        ics.push_str("END:VEVENT\r\n");
    }

    ics.push_str("END:VCALENDAR\r\n");
    ics
}

pub fn escape_ics(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace(';', "\\;")
     .replace(',', "\\,")
     .replace('\n', "\\n")
}

pub fn parse_ics_date(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 15 && s.contains('T') {
        let date_part = &s[0..8];
        let time_part = &s[9..15];
        let year = &date_part[0..4];
        let month = &date_part[4..6];
        let day = &date_part[6..8];
        let hour = &time_part[0..2];
        let min = &time_part[2..4];
        let sec = &time_part[4..6];
        format!("{}-{}-{}T{}:{}:{}Z", year, month, day, hour, min, sec)
    } else if s.len() >= 8 {
        let year = &s[0..4];
        let month = &s[4..6];
        let day = &s[6..8];
        format!("{}-{}-{}", year, month, day)
    } else {
        s.to_string()
    }
}

pub fn parse_ics_to_page_info(ics_content: &str, default_id: &str) -> PageInfo {
    let mut title = "(untitled)".to_string();
    let mut start = "".to_string();
    let mut end = None;
    let mut description = "".to_string();
    let mut uid = default_id.to_string();

    for line in ics_content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("SUMMARY:") {
            title = rest.replace("\\,", ",").replace("\\;", ";").replace("\\n", "\n").replace("\\\\", "\\");
        } else if let Some(rest) = line.strip_prefix("DTSTART:") {
            start = parse_ics_date(rest);
        } else if let Some(rest) = line.strip_prefix("DTSTART;VALUE=DATE:") {
            start = parse_ics_date(rest);
        } else if let Some(rest) = line.strip_prefix("DTEND:") {
            end = Some(parse_ics_date(rest));
        } else if let Some(rest) = line.strip_prefix("DTEND;VALUE=DATE:") {
            end = Some(parse_ics_date(rest));
        } else if let Some(rest) = line.strip_prefix("DESCRIPTION:") {
            description = rest.replace("\\,", ",").replace("\\;", ";").replace("\\n", "\n").replace("\\\\", "\\");
        } else if let Some(rest) = line.strip_prefix("UID:") {
            uid = rest.to_string();
        }
    }

    PageInfo {
        id: uid,
        title,
        start,
        end,
        url: description,
        last_edited: chrono::Utc::now().to_rfc3339(),
    }
}

pub fn matches_id(page_id: &str, target_id: &str) -> bool {
    let p_clean = page_id.replace("-", "").to_lowercase();
    let t_clean = target_id.replace("-", "").to_lowercase();
    p_clean == t_clean
}

pub fn build_propfind_calendar(prefix: &str, display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>{prefix}</D:href>
    <D:propstat>
      <D:prop>
        <D:displayname>{display_name}</D:displayname>
        <D:resourcetype>
          <D:collection/>
          <C:calendar/>
        </D:resourcetype>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
        </C:supported-calendar-component-set>
        <D:current-user-principal>
          <D:href>/principals/</D:href>
        </D:current-user-principal>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        prefix = prefix,
        display_name = display_name
    )
}

pub fn build_propfind_calendar_with_events(prefix: &str, display_name: &str, pages: &[PageInfo]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>"#);
    xml.push_str(prefix);
    xml.push_str(r#"</D:href>
    <D:propstat>
      <D:prop>
        <D:displayname>"#);
    xml.push_str(display_name);
    xml.push_str(r#"</D:displayname>
        <D:resourcetype>
          <D:collection/>
          <C:calendar/>
        </D:resourcetype>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
        </C:supported-calendar-component-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#);

    for page in pages {
        let clean_id = page.id.replace("-", "");
        let etag = &page.last_edited;
        let href = if prefix == "/" {
            format!("/{}.ics", clean_id)
        } else {
            format!("{}{}.ics", prefix, clean_id)
        };
        xml.push_str(&format!(
            r#"
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>
        <D:getetag>"{etag}"</D:getetag>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#,
            href = href,
            etag = etag
        ));
    }

    xml.push_str("\n</D:multistatus>");
    xml
}

pub fn build_propfind_event(prefix: &str, event_id: &str, page: &PageInfo) -> String {
    let clean_id = event_id.replace(".ics", "");
    let etag = &page.last_edited;
    let href = if prefix == "/" {
        format!("/{}.ics", clean_id)
    } else {
        format!("{}{}.ics", prefix, clean_id)
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>
        <D:getetag>"{etag}"</D:getetag>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        href = href,
        etag = etag
    )
}

pub fn build_report_response(db_id: &str, prefix: &str, calendar_name: &str, pages: &[PageInfo]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#);

    for page in pages {
        let clean_id = page.id.replace("-", "");
        let etag = &page.last_edited;
        let ics_body = build_ics(db_id, calendar_name, std::slice::from_ref(page));
        let href = if prefix == "/" {
            format!("/{}.ics", clean_id)
        } else {
            format!("{}{}.ics", prefix, clean_id)
        };
        xml.push_str(&format!(
            r#"
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:getetag>"{etag}"</D:getetag>
        <C:calendar-data><![CDATA[{ics_body}]]></C:calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#,
            href = href,
            etag = etag,
            ics_body = ics_body
        ));
    }

    xml.push_str("\n</D:multistatus>");
    xml
}

/// Resolves the legacy per-host calendar alias (calendar.opendiy.vn /
/// mytime.opendiy.vn — a single-owner shortcut that predates multi-tenancy)
/// to that calendar's public_id, so it flows through the exact same
/// auth-ownership and routing logic as path-based `/cal/{public_id}` access.
pub async fn get_public_id_for_host(headers: &axum::http::HeaderMap, state: &AppState) -> Option<String> {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("").trim();
    let db_id = match host_name {
        "calendar.opendiy.vn" => Some("4cb38c7656ae483d8ee5650d9fb02108"),
        "mytime.opendiy.vn" => Some("39e6a94a90a680da85d2c29e3c52ed8e"),
        _ => None,
    }?;
    state.calendar_by_db_id(db_id).await.map(|cal| cal.public_id)
}

pub async fn handle_calendar_impl(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    state: AppState,
    public_id: String,
    prefix: String,
    body: String,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let Some(cal) = state.calendar_by_public_id(&public_id).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let name = if cal.display_name.is_empty() {
        state.get_calendar_name(&cal.database_id, &cal.notion_access_token).await
    } else {
        cal.display_name.clone()
    };
    if method == axum::http::Method::PUT || method == axum::http::Method::DELETE || method.as_str() == "PROPPATCH" {
        if state.caldav_allow_writes != CaldavAllowWrites::True {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    info!(
        method = ?method,
        path = %prefix,
        host = %host,
        public_id = %public_id,
        database_id = %cal.database_id,
        calendar = %name,
        "CalDAV handler: calendar collection"
    );
    if method == axum::http::Method::OPTIONS {
        return axum::http::StatusCode::OK.into_response();
    }
    if method == axum::http::Method::GET {
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        let body = build_ics(&public_id, &name, &pages);
        return ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response();
    }

    if method.as_str() == "PROPFIND" {
        let depth = headers
            .get("depth")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("0");
        let body = if depth == "1" {
            let cache = state.cache.read().await;
            let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
            build_propfind_calendar_with_events(&prefix, &name, &pages)
        } else {
            build_propfind_calendar(&prefix, &name)
        };
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        ).into_response();
    }

    if method.as_str() == "PROPPATCH" {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>{prefix}</D:href>
    <D:propstat>
      <D:prop>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
        </C:supported-calendar-component-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
            prefix = prefix
        );
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        ).into_response();
    }

    if method.as_str() == "REPORT" {
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        let body = build_report_response(&public_id, &prefix, &name, &pages);
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        ).into_response();
    }

    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

pub async fn handle_calendar_event_impl(
    method: axum::http::Method,
    state: AppState,
    public_id: String,
    event_id: String,
    prefix: String,
    body: String,
) -> impl IntoResponse {
    let Some(cal) = state.calendar_by_public_id(&public_id).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let name = if cal.display_name.is_empty() {
        state.get_calendar_name(&cal.database_id, &cal.notion_access_token).await
    } else {
        cal.display_name.clone()
    };
    let event_id_clean = event_id.strip_suffix(".ics").unwrap_or(&event_id).to_string();
    if method == axum::http::Method::PUT || method == axum::http::Method::DELETE || method.as_str() == "PROPPATCH" {
        if state.caldav_allow_writes != CaldavAllowWrites::True {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    info!(
        method = ?method,
        path = %prefix,
        public_id = %public_id,
        database_id = %cal.database_id,
        event_id = %event_id_clean,
        calendar = %name,
        "CalDAV handler: calendar event"
    );
    if method == axum::http::Method::OPTIONS {
        return axum::http::StatusCode::OK.into_response();
    }
    if method == axum::http::Method::GET {
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_ics(&public_id, &name, std::slice::from_ref(page));
            info!(status=200, found=true, "CalDAV event GET");
            return ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response();
        } else {
            info!(status=404, found=false, "CalDAV event GET not found");
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    if method.as_str() == "PROPFIND" {
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_propfind_event(&prefix, &event_id_clean, page);
            return (
                axum::http::StatusCode::MULTI_STATUS,
                [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
                body,
            ).into_response();
        } else {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    if method == axum::http::Method::PUT {
        // Writes through to Notion (create or update, mirroring the
        // webview's handle_create_event/handle_update_event) instead of
        // just mutating the local cache — a cache-only write used to get
        // silently discarded on the very next refresh_all()/webhook-driven
        // refresh, since Notion is the source of truth for that cache.
        // Known limitation: a brand-new event's resource URL (based on the
        // CalDAV client's own generated UID) won't match the href the
        // server advertises on the next PROPFIND (based on the real Notion
        // page id) — the client may see it as a second, separate resource
        // until it next fetches the full listing.
        let new_page = parse_ics_to_page_info(&body, &event_id_clean);
        let existing_id = {
            let cache = state.cache.read().await;
            cache
                .get(&cal.database_id)
                .and_then(|pages| pages.iter().find(|p| matches_id(&p.id, &event_id_clean)).map(|p| p.id.clone()))
        };
        let result = if let Some(page_id) = existing_id {
            state
                .notion_update_event(
                    &page_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    Some(&new_page.title),
                    Some(&new_page.start),
                    Some(new_page.end.as_deref()),
                )
                .await
                .map(|_| axum::http::StatusCode::NO_CONTENT)
        } else {
            state
                .notion_create_event(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    &new_page.title,
                    &new_page.start,
                    new_page.end.as_deref(),
                )
                .await
                .map(|_page_id| axum::http::StatusCode::CREATED)
        };
        return match result {
            Ok(status) => {
                state.refresh_by_data_source(&cal.data_source_id).await;
                status.into_response()
            }
            Err(e) => {
                error!("CalDAV PUT event {} failed to sync to Notion: {}", event_id_clean, e);
                axum::http::StatusCode::BAD_GATEWAY.into_response()
            }
        };
    }

    if method == axum::http::Method::DELETE {
        let existing_id = {
            let cache = state.cache.read().await;
            cache
                .get(&cal.database_id)
                .and_then(|pages| pages.iter().find(|p| matches_id(&p.id, &event_id_clean)).map(|p| p.id.clone()))
        };
        let Some(page_id) = existing_id else {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        };
        return match state.notion_delete_event(&page_id, &cal.notion_access_token).await {
            Ok(()) => {
                state.refresh_by_data_source(&cal.data_source_id).await;
                axum::http::StatusCode::NO_CONTENT.into_response()
            }
            Err(e) => {
                error!("CalDAV DELETE event {} failed to sync to Notion: {}", event_id_clean, e);
                axum::http::StatusCode::BAD_GATEWAY.into_response()
            }
        };
    }

    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

/// Extracts (username, password) from an HTTP Basic Authorization header.
fn extract_basic_auth(headers: &axum::http::HeaderMap) -> Option<(String, String)> {
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok())?;
    let basic_val = auth_header.strip_prefix("Basic ")?;
    let decoded = base64_light::base64_decode_str(basic_val);
    let mut parts = decoded.splitn(2, ':');
    let username = parts.next()?.to_string();
    let password = parts.next()?.to_string();
    Some((username, password))
}

/// Pulls `{public_id}` out of a `/cal/{public_id}/...` request path, used to
/// confirm the authenticated calendar's own public_id matches the one being
/// requested (a valid credential for calendar A must not open calendar B).
fn extract_path_public_id(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/cal/")?;
    let seg = rest.split('/').next().unwrap_or("");
    if seg.is_empty() { None } else { Some(seg.to_string()) }
}

pub async fn handle_path_calendar(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(public_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", public_id);
    let res = handle_calendar_impl(method, headers, state, public_id, prefix, body).await.into_response();
    add_caldav_headers(res)
}

pub async fn handle_path_calendar_event(
    method: axum::http::Method,
    State(state): State<AppState>,
    Path((public_id, event_id)): Path<(String, String)>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", public_id);
    let res = handle_calendar_event_impl(method, state, public_id, event_id, prefix, body).await.into_response();
    add_caldav_headers(res)
}

pub async fn handle_host_calendar(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let host_public_id = get_public_id_for_host(&headers, &state).await;
    info!(
        method = ?method,
        path = "/",
        host = %host,
        host_public_id = ?host_public_id,
        "CalDAV handler: host calendar root"
    );
    if let Some(public_id) = host_public_id {
        let prefix = "/".to_string();
        let res = handle_calendar_impl(method, headers, state, public_id, prefix, body).await.into_response();
        add_caldav_headers(res)
    } else {
        if method == axum::http::Method::GET || method == axum::http::Method::HEAD {
            return crate::auth::landing_page().await.into_response();
        }
        if method == axum::http::Method::OPTIONS {
            return axum::http::StatusCode::OK.into_response();
        }
        if method.as_str() == "PROPFIND" {
            let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/</D:href>
    <D:propstat>
      <D:prop>
        <D:current-user-principal>
          <D:href>/principals/</D:href>
        </D:current-user-principal>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;
            return (
                axum::http::StatusCode::MULTI_STATUS,
                [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
                body,
            ).into_response();
        }
        axum::http::StatusCode::NOT_FOUND.into_response()
    }
}

pub async fn handle_host_calendar_event(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(event_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    if let Some(public_id) = get_public_id_for_host(&headers, &state).await {
        let prefix = "/".to_string();
        let event_id_clean = event_id.strip_suffix(".ics").unwrap_or(&event_id);
        info!(
            method = ?method,
            path = "/",
            host = %host,
            public_id = %public_id,
            event_id = %event_id_clean,
            "CalDAV handler: host calendar event"
        );
        let res = handle_calendar_event_impl(method, state, public_id, event_id, prefix, body).await.into_response();
        add_caldav_headers(res)
    } else {
        info!(
            method = ?method,
            path = "/",
            host = %host,
            event_id = %event_id,
            status = 404,
            "CalDAV handler: host calendar event - no host db"
        );
        axum::http::StatusCode::NOT_FOUND.into_response()
    }
}

fn add_caldav_headers(mut response: axum::response::Response) -> axum::response::Response {
    let headers = response.headers_mut();
    headers.insert("DAV", axum::http::HeaderValue::from_static("1, 3, calendar-access"));
    headers.insert("Allow", axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH"));
    response
}

// Fallback/catch-all or custom route handlers for new endpoints with Auth.
async fn handle_well_known(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let query = headers.get("x-request-query").map(|_| "has-query").unwrap_or("").to_string();
    info!(
        method = ?method,
        path = "/.well-known/caldav",
        host = %host,
        query = %query,
        "Discovery: /.well-known/caldav"
    );
    (
        axum::http::StatusCode::MOVED_PERMANENTLY,
        [
            (header::LOCATION, axum::http::HeaderValue::from_static("/principals/")),
            (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
            (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
        ],
    )
}

async fn handle_principals(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    auth: Option<axum::Extension<AuthenticatedCaldavUser>>,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    info!(
        method = ?method,
        path = "/principals/",
        host = %host,
        "Discovery: /principals/"
    );
    if method == axum::http::Method::OPTIONS {
        return (
            axum::http::StatusCode::OK,
            [
                (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
                (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
            ],
        ).into_response();
    }
    if method.as_str() == "PROPFIND" {
        let username = auth.map(|a| a.0.username.clone()).unwrap_or_else(|| "user".to_string());
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/principals/</D:href>
    <D:propstat>
      <D:prop>
        <D:current-user-principal>
          <D:href>/principals/</D:href>
        </D:current-user-principal>
        <C:calendar-home-set>
          <D:href>/calendars/{username}/</D:href>
        </C:calendar-home-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
            username = username
        );
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [
                (header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/xml; charset=utf-8")),
                (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
                (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
            ],
            body,
        ).into_response();
    }
    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

async fn handle_calendars_propfind(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(_user): Path<String>,
    auth: Option<axum::Extension<AuthenticatedCaldavUser>>,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let depth = headers
        .get("depth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0")
        .to_string();
    info!(
        method = ?method,
        path = "/calendars/{user}",
        host = %host,
        depth = %depth,
        user = %_user,
        "CalDAV handler: /calendars collection"
    );
    if method == axum::http::Method::OPTIONS {
        return (
            axum::http::StatusCode::OK,
            [
                (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
                (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
            ],
        ).into_response();
    }
    // Scoped to the *specific calendar* these credentials belong to — not
    // just the app-account owner. Each calendar has its own generated
    // caldav_username/password (see oauth.rs::create_calendars), so a user
    // with several calendars gets several independent credential pairs. A
    // prior version of this filtered by `user_id` alone, which listed every
    // one of that user's calendars regardless of which pair authenticated —
    // discovery would hand a client hrefs for calendars it doesn't hold
    // credentials for, and it would then dutifully try (and get 403 on) all
    // of them. Confirmed live in production logs: Apple Calendar retrying
    // PROPFIND/PROPPATCH every ~30s against two calendars it wasn't
    // authenticated for, using a third calendar's credentials.
    let owner_calendars = match &auth {
        Some(a) => state
            .calendars_for_user(a.0.user_id)
            .await
            .into_iter()
            .filter(|c| c.caldav_username == a.0.username)
            .collect(),
        None => Vec::new(),
    };

    if method.as_str() == "PROPFIND" {
        let host_public_id = get_public_id_for_host(&headers, &state).await;
        let all_cals = owner_calendars;
        let cals_to_return: Vec<CalendarRow> = if let Some(public_id) = &host_public_id {
            all_cals.into_iter().filter(|c| &c.public_id == public_id).collect()
        } else {
            all_cals
        };

        let mut responses_xml = String::new();
        for cal in cals_to_return {
            let name = if cal.display_name.is_empty() {
                state.get_calendar_name(&cal.database_id, &cal.notion_access_token).await
            } else {
                cal.display_name.clone()
            };
            let href = if host_public_id.is_some() {
                "/".to_string()
            } else {
                format!("/cal/{}/", cal.public_id)
            };

            responses_xml.push_str(&format!(
                r#"  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:displayname>{name}</D:displayname>
        <D:resourcetype>
          <D:collection/>
          <C:calendar/>
        </D:resourcetype>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
        </C:supported-calendar-component-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
"#,
                href = href,
                name = name
            ));
        }

        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
{responses_xml}</D:multistatus>"#,
            responses_xml = responses_xml
        );
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [
                (header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/xml; charset=utf-8")),
                (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
                (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
            ],
            body,
        ).into_response();
    }
    if method.as_str() == "REPORT" {
        let host_public_id = get_public_id_for_host(&headers, &state).await;
        let all_cals = owner_calendars;
        let cals_to_return: Vec<CalendarRow> = if let Some(public_id) = &host_public_id {
            all_cals.into_iter().filter(|c| &c.public_id == public_id).collect()
        } else {
            all_cals
        };

        let mut xml = String::new();
        xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#);

        let cache = state.cache.read().await;
        for cal in cals_to_return {
            let name = if cal.display_name.is_empty() {
                state.get_calendar_name(&cal.database_id, &cal.notion_access_token).await
            } else {
                cal.display_name.clone()
            };
            let prefix = if host_public_id.is_some() {
                "/".to_string()
            } else {
                format!("/cal/{}/", cal.public_id)
            };
            let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
            for page in pages {
                let clean_id = page.id.replace("-", "");
                let etag = &page.last_edited;
                let ics_body = build_ics(&cal.public_id, &name, std::slice::from_ref(&page));
                let href = format!("{}{}.ics", prefix, clean_id);
                xml.push_str(&format!(
                    r#"
  <D:response>
    <D:href>{href}</D:href>
    <D:propstat>
      <D:prop>
        <D:getetag>"{etag}"</D:getetag>
        <C:calendar-data><![CDATA[{ics_body}]]></C:calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#,
                    href = href,
                    etag = etag,
                    ics_body = ics_body
                ));
            }
        }
        xml.push_str("\n</D:multistatus>");

        return (
            axum::http::StatusCode::MULTI_STATUS,
            [
                (header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/xml; charset=utf-8")),
                (axum::http::HeaderName::from_static("dav"), axum::http::HeaderValue::from_static("1, 3, calendar-access")),
                (axum::http::HeaderName::from_static("allow"), axum::http::HeaderValue::from_static("GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH")),
            ],
            xml,
        ).into_response();
    }
    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

// Authentication middleware wrapper — every CalDAV request needs a Basic
// Auth credential matching some calendar's own caldav_username/password
// (looked up in Postgres). There's no "auth disabled" bypass anymore: unlike
// the single-tenant env-var scheme this replaced, every calendar has real
// per-tenant credentials from the moment it's created via onboarding.
async fn auth_middleware(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    info!(
        method = ?method,
        path = %path,
        host = %host,
        query = %query,
        "Incoming CalDAV request"
    );

    if path == "/.well-known/caldav" {
        info!("Discovery flow step: /.well-known/caldav redirect");
    } else if path.starts_with("/principals") {
        info!("Discovery flow step: /principals/");
    } else if path.starts_with("/calendars") {
        info!("Discovery flow step: /calendars/");
    }

    let start = std::time::Instant::now();

    // Only OPTIONS (CORS preflight, no calendar data in the response) skips
    // auth. GET/PROPFIND/REPORT used to bypass here too, which meant every
    // "protected" CalDAV route was actually readable by anyone — reads are
    // exactly what needs protecting, not just writes.
    //
    // The one other bypass: a bare GET/HEAD "/" on a host with no personal
    // calendar alias (see get_public_id_for_host) isn't a CalDAV request at
    // all — it's a browser hitting the marketing domain, which should see
    // the landing page, not a Basic Auth prompt. calendar.opendiy.vn/
    // mytime.opendiy.vn (real personal calendar aliases) are unaffected
    // since get_public_id_for_host returns Some for those hosts.
    let is_landing_page_request = (method == axum::http::Method::GET || method == axum::http::Method::HEAD)
        && path == "/"
        && get_public_id_for_host(&headers, &state).await.is_none();
    let is_bypass = method == axum::http::Method::OPTIONS || is_landing_page_request;

    if is_bypass {
        let mut response = next.run(request).await;
        response = add_caldav_headers(response);
        let duration = start.elapsed();
        info!(
            method = ?method,
            path = %path,
            status = response.status().as_u16(),
            duration_ms = duration.as_millis(),
            "CalDAV request completed"
        );
        return response;
    }

    let unauthorized_response = || {
        add_caldav_headers(
            (
                axum::http::StatusCode::UNAUTHORIZED,
                [
                    (header::WWW_AUTHENTICATE, "Basic realm=\"CalDAV Server\""),
                    (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                ],
                "Unauthorized",
            )
                .into_response(),
        )
    };

    let Some((username, password)) = extract_basic_auth(&headers) else {
        info!("Authentication failure: no Basic Auth credentials presented");
        let response = unauthorized_response();
        let duration = start.elapsed();
        info!(
            method = ?method,
            path = %path,
            status = response.status().as_u16(),
            duration_ms = duration.as_millis(),
            "CalDAV request completed"
        );
        return response;
    };

    let Some((user_id, auth_public_id)) = state.verify_caldav_credentials(&username, &password).await else {
        info!(username = %username, "Authentication failure: invalid credentials");
        let response = unauthorized_response();
        let duration = start.elapsed();
        info!(
            method = ?method,
            path = %path,
            status = response.status().as_u16(),
            duration_ms = duration.as_millis(),
            "CalDAV request completed"
        );
        return response;
    };

    // Path-scoped (/cal/{public_id}/...) or legacy host-based requests must
    // match the authenticated calendar's own public_id — otherwise user A's
    // valid credentials could read/write user B's calendar just by knowing
    // its id (or, now that database_id can be shared by multiple users'
    // rows, by knowing a database_id someone else also subscribed to).
    let target_public_id = match extract_path_public_id(&path) {
        Some(id) => Some(id),
        None => get_public_id_for_host(&headers, &state).await,
    };
    if let Some(target) = &target_public_id {
        if target != &auth_public_id {
            info!(username = %username, target_public_id = %target, "Authorization failure: calendar not owned by these credentials");
            let response = add_caldav_headers(
                (
                    axum::http::StatusCode::FORBIDDEN,
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    "Forbidden",
                )
                    .into_response(),
            );
            let duration = start.elapsed();
            info!(
                method = ?method,
                path = %path,
                status = response.status().as_u16(),
                duration_ms = duration.as_millis(),
                "CalDAV request completed"
            );
            return response;
        }
    }

    info!(username = %username, "Authentication success");
    request.extensions_mut().insert(AuthenticatedCaldavUser { user_id, username });

    let mut response = next.run(request).await;
    response = add_caldav_headers(response);
    let duration = start.elapsed();
    info!(
        method = ?method,
        path = %path,
        status = response.status().as_u16(),
        duration_ms = duration.as_millis(),
        "CalDAV request completed"
    );
    response
}

pub fn create_app(
    state: AppState,
    oidc_client: axum_oidc::OidcClient<axum_oidc::EmptyAdditionalClaims>,
    app_config: crate::auth::AppConfig,
) -> Router {
    use axum::error_handling::HandleErrorLayer;
    use axum_oidc::{error::MiddlewareError, handle_oidc_redirect, EmptyAdditionalClaims, OidcAuthLayer, OidcLoginLayer};
    use crate::auth::SessionWrapper;
    use tower::ServiceBuilder;

    let oidc_login_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move { e.into_response() }))
        .layer(OidcLoginLayer::<EmptyAdditionalClaims, SessionWrapper>::new());

    let oidc_auth_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move { e.into_response() }))
        .layer(OidcAuthLayer::<_, SessionWrapper>::new(oidc_client));

    let caldav_routes = Router::<AppState>::new()
        .route(
            "/cal/{public_id}",
            axum::routing::any(handle_path_calendar),
        )
        .route(
            "/cal/{public_id}/",
            axum::routing::any(handle_path_calendar),
        )
        .route(
            "/cal/{public_id}/{event_id}",
            axum::routing::any(handle_path_calendar_event),
        )
        .route(
            "/cal/{public_id}/{event_id}/",
            axum::routing::any(handle_path_calendar_event),
        )
        .route(
            "/.well-known/caldav",
            axum::routing::any(handle_well_known),
        )
        .route(
            "/.well-known/caldav/",
            axum::routing::any(handle_well_known),
        )
        .route(
            "/principals",
            axum::routing::any(handle_principals),
        )
        .route(
            "/principals/",
            axum::routing::any(handle_principals),
        )
        .route(
            "/calendars/{user}",
            axum::routing::any(handle_calendars_propfind),
        )
        .route(
            "/calendars/{user}/",
            axum::routing::any(handle_calendars_propfind),
        )
        .route(
            "/",
            axum::routing::any(handle_host_calendar),
        )
        .route(
            "/{event_id}",
            axum::routing::any(handle_host_calendar_event),
        )
        .route(
            "/{event_id}/",
            axum::routing::any(handle_host_calendar_event),
        )
        // Both moved here (from the unauthenticated router below) since they
        // either leak calendar data (/cal.ics) or let anyone force a Notion
        // API call (/refresh) — same auth requirement as the CalDAV routes.
        // Both scoped to the AuthenticatedCaldavUser the middleware resolved
        // — this used to operate over *every* tenant's calendars given any
        // one valid credential, a real cross-tenant leak/abuse vector.
        .route("/refresh", post(move |axum::Extension(auth): axum::Extension<AuthenticatedCaldavUser>, State(state): State<AppState>| async move {
            state.refresh_for_user(auth.user_id).await;
            "refresh triggered"
        }))
        .route(
            "/cal.ics",
            get(move |axum::Extension(auth): axum::Extension<AuthenticatedCaldavUser>, State(state): State<AppState>| async move {
                let my_cals = state.calendars_for_user(auth.user_id).await;
                let cache = state.cache.read().await;
                let mut all_pages: Vec<PageInfo> = Vec::new();
                let mut names: Vec<String> = Vec::new();
                for cal in &my_cals {
                    if let Some(pages) = cache.get(&cal.database_id) {
                        all_pages.extend(pages.clone());
                    }
                    names.push(if cal.display_name.is_empty() {
                        format!("Notion {}", &cal.database_id[..8])
                    } else {
                        cal.display_name.clone()
                    });
                }
                let name = names.join(", ");
                let body = build_ics("all", &name, &all_pages);
                ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response()
            }),
        )
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

    // Built as its own router (not chained onto the others before calling
    // .layer()) because Router::layer() wraps *every* route already
    // registered on that router, not just the one added right before it —
    // chaining this after /health and /webhook/notion-test made those force
    // a Keycloak login redirect too, which isn't what "/me forces login" was
    // supposed to mean.
    let me_route = Router::new()
        .route("/me", get(crate::auth::me))
        .route("/connect/notion", get(crate::oauth::connect_notion_page))
        .route("/connect/notion/start", get(crate::oauth::connect_notion_start))
        .route(
            "/connect/notion/databases",
            get(crate::oauth::pick_databases_page).post(crate::oauth::create_calendars),
        )
        .route("/oauth/notion/callback", get(crate::oauth::notion_oauth_callback))
        .route("/me/calendars/{public_id}/delete", post(crate::oauth::delete_calendar))
        .route("/me/calendars/{public_id}/reveal-password", post(crate::oauth::reveal_password))
        .route("/me/calendars/{public_id}/regenerate-password", post(crate::oauth::regenerate_password))
        // Webview: server-rendered FullCalendar page + its JSON CRUD API,
        // writing straight through to Notion (see notion_create/update/
        // delete_event on AppState). Was CalDAV-Basic-Auth-protected
        // before, alongside routes meant for calendar apps, not browsers —
        // now gated the same way as the rest of the dashboard (OIDC
        // session), with per-handler ownership checks against the logged-in
        // user's own calendars.
        // Superseded by /me (the real dashboard, with credentials/cards) —
        // kept as a redirect so old bookmarks/links to the bare index still land somewhere useful.
        .route("/app", get(|| async { axum::response::Redirect::to("/me") }))
        .route("/app/{public_id}", get(crate::webview::handle_webview_page))
        .route("/app/{public_id}/api/events", get(crate::webview::handle_list_events).post(crate::webview::handle_create_event))
        .route(
            "/app/{public_id}/api/events/{event_id}",
            axum::routing::patch(crate::webview::handle_update_event).delete(crate::webview::handle_delete_event),
        )
        .layer(oidc_login_service);

    Router::new()
        .route(
            "/health",
            get(move |State(state): State<AppState>| async move {
                axum::Json(serde_json::json!({
                    "status": "ok",
                    "caldav_allow_writes": state.caldav_allow_writes
                }))
            })
            .layer(CorsLayer::permissive()),
        )
        // Not auth-protected: Notion can't send Basic Auth credentials, so
        // this is instead authenticated via HMAC signature verification
        // inside the handler itself (see webhook.rs).
        .route(
            "/webhook/notion-test",
            post(crate::webhook::handle_notion_webhook),
        )
        // Public/unauthenticated: linked from the Notion OAuth consent step
        // and the dashboard footer, and required for eventual Notion
        // Marketplace submission.
        .route("/privacy", get(crate::legal::privacy_policy_page))
        .route("/terms", get(crate::legal::terms_of_service_page))
        // SaaS login (Keycloak) — separate identity from the CalDAV Basic
        // Auth / Notion OAuth above. /me forces login via oidc_login_service;
        // /oidc (callback) and /logout must NOT themselves force a redirect,
        // so they sit outside that layer.
        .merge(me_route)
        .route(
            "/oidc",
            axum::routing::any(handle_oidc_redirect::<EmptyAdditionalClaims, SessionWrapper>),
        )
        .route("/logout", get(crate::auth::logout))
        .merge(caldav_routes)
        // Applied last so it wraps everything above — only populates claims
        // when a session exists, never forces a redirect itself (that's
        // oidc_login_service's job, scoped to /me only).
        .layer(oidc_auth_service)
        .layer(axum::Extension(app_config))
        .with_state(state)
        // Outermost layer: logs method/path/status/latency for every request
        // across the whole app (webview, OIDC, oauth, legal, CalDAV — not
        // just the routes that already had their own manual info!() calls).
        // Custom on_request/on_response closures instead of tower_http's
        // DefaultOnRequest/DefaultOnResponse: those log method/uri only as
        // span fields, which tracing_subscriber::fmt's default formatter
        // doesn't print inline, so they were invisible in `kubectl logs`.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(false))
                .on_request(|request: &axum::http::Request<axum::body::Body>, _span: &tracing::Span| {
                    info!(method = %request.method(), uri = %request.uri(), "request started");
                })
                .on_response(
                    |response: &axum::http::Response<axum::body::Body>, latency: std::time::Duration, _span: &tracing::Span| {
                        let status = response.status();
                        if status.is_server_error() {
                            error!(status = %status, latency_ms = %latency.as_millis(), "request completed");
                        } else {
                            info!(status = %status, latency_ms = %latency.as_millis(), "request completed");
                        }
                    },
                ),
        )
}
