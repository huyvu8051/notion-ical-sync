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
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
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
    pub database_id: String,
    pub data_source_id: String,
    pub date_property: String,
    pub display_name: String,
    pub notion_access_token: String,
}

impl AppState {
    pub fn new(
        db: PgPool,
        caldav_allow_writes: CaldavAllowWrites,
        webhook_secret: Option<String>,
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
        }
    }

    pub async fn all_calendars(&self) -> Vec<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.database_id, c.data_source_id, c.date_property, c.display_name, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id",
        )
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to list calendars from db: {}", e);
            Vec::new()
        })
    }

    pub async fn calendar_by_db_id(&self, db_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(
            "SELECT c.database_id, c.data_source_id, c.date_property, c.display_name, nc.notion_access_token
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.database_id = $1",
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
            "SELECT c.database_id, c.data_source_id, c.date_property, c.display_name, nc.notion_access_token
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

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(format!("Notion error {}: {}", status, txt));
        }

        let data: NotionQueryResponse = resp
            .json()
            .await
            .map_err(|e| format!("Parse failed: {}", e))?;

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
        let mut cache = self.cache.write().await;
        for cal in calendars {
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

        let resp = self
            .client
            .post("https://api.notion.com/v1/pages")
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(format!("Notion error {}: {}", status, txt));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| format!("Parse failed: {}", e))?;
        data.get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
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
        let resp = self
            .client
            .patch(format!("https://api.notion.com/v1/pages/{}", page_id))
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(format!("Notion error {}: {}", status, txt));
        }
        Ok(())
    }

    pub async fn get_calendar_name(&self, db_id: &str, notion_token: &str) -> String {
        match self.client
            .get(format!("https://api.notion.com/v1/databases/{}", db_id))
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                r.json::<serde_json::Value>().await
                    .ok()
                    .and_then(|v| v.get("title").cloned())
                    .and_then(|t| {
                        let arr = t.as_array()?;
                        let item = arr.first()?;
                        let txt = item.get("plain_text")?;
                        txt.as_str().map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| format!("Notion {}", &db_id[..8]))
            }
            _ => format!("Notion {}", &db_id[..8]),
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

pub async fn get_db_id_for_host(headers: &axum::http::HeaderMap, state: &AppState) -> Option<String> {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("").trim();
    let db_id = match host_name {
        "calendar.opendiy.vn" => Some("4cb38c7656ae483d8ee5650d9fb02108".to_string()),
        "mytime.opendiy.vn" => Some("39e6a94a90a680da85d2c29e3c52ed8e".to_string()),
        _ => None,
    };
    match db_id {
        Some(id) if state.calendar_by_db_id(&id).await.is_some() => Some(id),
        _ => None,
    }
}

pub async fn handle_calendar_impl(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    state: AppState,
    db_id: String,
    prefix: String,
    body: String,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let Some(cal) = state.calendar_by_db_id(&db_id).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let name = if cal.display_name.is_empty() {
        state.get_calendar_name(&db_id, &cal.notion_access_token).await
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
        db_id = %db_id,
        calendar = %name,
        "CalDAV handler: calendar collection"
    );
    if method == axum::http::Method::GET {
        let cache = state.cache.read().await;
        let pages = cache.get(&db_id).cloned().unwrap_or_default();
        let body = build_ics(&db_id, &name, &pages);
        return ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response();
    }

    if method.as_str() == "PROPFIND" {
        let depth = headers
            .get("depth")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("0");
        let body = if depth == "1" {
            let cache = state.cache.read().await;
            let pages = cache.get(&db_id).cloned().unwrap_or_default();
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
        let pages = cache.get(&db_id).cloned().unwrap_or_default();
        let body = build_report_response(&db_id, &prefix, &name, &pages);
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
    db_id: String,
    event_id: String,
    prefix: String,
    body: String,
) -> impl IntoResponse {
    let Some(cal) = state.calendar_by_db_id(&db_id).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let name = if cal.display_name.is_empty() {
        state.get_calendar_name(&db_id, &cal.notion_access_token).await
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
        db_id = %db_id,
        event_id = %event_id_clean,
        calendar = %name,
        "CalDAV handler: calendar event"
    );
    if method == axum::http::Method::GET {
        let cache = state.cache.read().await;
        let pages = cache.get(&db_id).cloned().unwrap_or_default();
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_ics(&db_id, &name, std::slice::from_ref(page));
            info!(status=200, found=true, "CalDAV event GET");
            return ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response();
        } else {
            info!(status=404, found=false, "CalDAV event GET not found");
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    if method.as_str() == "PROPFIND" {
        let cache = state.cache.read().await;
        let pages = cache.get(&db_id).cloned().unwrap_or_default();
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
        let new_page = parse_ics_to_page_info(&body, &event_id_clean);
        let mut cache = state.cache.write().await;
        let pages = cache.entry(db_id).or_default();
        if let Some(pos) = pages.iter().position(|p| matches_id(&p.id, &event_id_clean)) {
            pages[pos] = new_page;
            return axum::http::StatusCode::NO_CONTENT.into_response();
        } else {
            pages.push(new_page);
            return axum::http::StatusCode::CREATED.into_response();
        }
    }

    if method == axum::http::Method::DELETE {
        let mut cache = state.cache.write().await;
        if let Some(pages) = cache.get_mut(&db_id) {
            if let Some(pos) = pages.iter().position(|p| matches_id(&p.id, &event_id_clean)) {
                pages.remove(pos);
                return axum::http::StatusCode::NO_CONTENT.into_response();
            }
        }
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }

    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

// Helper to check HTTP Basic Auth using env variables CALDAV_USERNAME/CALDAV_PASSWORD.
// If either CALDAV_USERNAME or CALDAV_PASSWORD are not set, auth is disabled/bypassed.
pub fn check_auth(headers: &axum::http::HeaderMap) -> bool {
    let username_env = std::env::var("CALDAV_USERNAME").unwrap_or_default();
    let password_env = std::env::var("CALDAV_PASSWORD").unwrap_or_default();
    if username_env.is_empty() || password_env.is_empty() {
        return true;
    }

    if let Some(auth_header) = headers.get("Authorization").and_then(|h| h.to_str().ok()) {
        if let Some(basic_val) = auth_header.strip_prefix("Basic ") {
            let decoded = base64_light::base64_decode_str(basic_val);
            let parts: Vec<&str> = decoded.splitn(2, ':').collect();
            if parts.len() == 2 {
                return parts[0] == username_env && parts[1] == password_env;
            }
        }
    }
    false
}

pub async fn handle_path_calendar(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(db_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", db_id);
    let res = handle_calendar_impl(method, headers, state, db_id, prefix, body).await.into_response();
    add_caldav_headers(res)
}

pub async fn handle_path_calendar_event(
    method: axum::http::Method,
    State(state): State<AppState>,
    Path((db_id, event_id)): Path<(String, String)>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", db_id);
    let res = handle_calendar_event_impl(method, state, db_id, event_id, prefix, body).await.into_response();
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
    let host_db_id = get_db_id_for_host(&headers, &state).await;
    info!(
        method = ?method,
        path = "/",
        host = %host,
        host_db_id = ?host_db_id,
        "CalDAV handler: host calendar root"
    );
    if let Some(db_id) = host_db_id {
        let prefix = "/".to_string();
        let res = handle_calendar_impl(method, headers, state, db_id, prefix, body).await.into_response();
        add_caldav_headers(res)
    } else {
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
    if let Some(db_id) = get_db_id_for_host(&headers, &state).await {
        let prefix = "/".to_string();
        let event_id_clean = event_id.strip_suffix(".ics").unwrap_or(&event_id);
        info!(
            method = ?method,
            path = "/",
            host = %host,
            db_id = %db_id,
            event_id = %event_id_clean,
            "CalDAV handler: host calendar event"
        );
        let res = handle_calendar_event_impl(method, state, db_id, event_id, prefix, body).await.into_response();
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
        let username = std::env::var("CALDAV_USERNAME").unwrap_or_else(|_| "user".to_string());
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
    if method.as_str() == "PROPFIND" {
        let host_db_id = get_db_id_for_host(&headers, &state).await;
        let all_cals = state.all_calendars().await;
        let cals_to_return: Vec<CalendarRow> = if let Some(db_id) = &host_db_id {
            all_cals.into_iter().filter(|c| &c.database_id == db_id).collect()
        } else {
            all_cals
        };

        let mut responses_xml = String::new();
        for cal in cals_to_return {
            let db_id = cal.database_id.clone();
            let name = if cal.display_name.is_empty() {
                state.get_calendar_name(&db_id, &cal.notion_access_token).await
            } else {
                cal.display_name.clone()
            };
            let href = if host_db_id.is_some() {
                "/".to_string()
            } else {
                format!("/cal/{}/", db_id)
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
        let host_db_id = get_db_id_for_host(&headers, &state).await;
        let all_cals = state.all_calendars().await;
        let cals_to_return: Vec<CalendarRow> = if let Some(db_id) = &host_db_id {
            all_cals.into_iter().filter(|c| &c.database_id == db_id).collect()
        } else {
            all_cals
        };

        let mut xml = String::new();
        xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#);

        let cache = state.cache.read().await;
        for cal in cals_to_return {
            let db_id = cal.database_id.clone();
            let name = if cal.display_name.is_empty() {
                state.get_calendar_name(&db_id, &cal.notion_access_token).await
            } else {
                cal.display_name.clone()
            };
            let prefix = if host_db_id.is_some() {
                "/".to_string()
            } else {
                format!("/cal/{}/", db_id)
            };
            let pages = cache.get(&db_id).cloned().unwrap_or_default();
            for page in pages {
                let clean_id = page.id.replace("-", "");
                let etag = &page.last_edited;
                let ics_body = build_ics(&db_id, &name, std::slice::from_ref(&page));
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

fn extract_username(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers.get("Authorization").and_then(|h| h.to_str().ok()) {
        if let Some(basic_val) = auth_header.strip_prefix("Basic ") {
            let decoded = base64_light::base64_decode_str(basic_val);
            let parts: Vec<&str> = decoded.splitn(2, ':').collect();
            if !parts.is_empty() {
                return Some(parts[0].to_string());
            }
        }
    }
    None
}

// Authentication middleware wrapper
async fn auth_middleware(
    headers: axum::http::HeaderMap,
    request: axum::extract::Request,
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
    let is_bypass = method == axum::http::Method::OPTIONS;

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

    let is_authed = check_auth(&headers);
    let username = extract_username(&headers);

    let username_env = std::env::var("CALDAV_USERNAME").unwrap_or_default();
    let password_env = std::env::var("CALDAV_PASSWORD").unwrap_or_default();
    let auth_enabled = !username_env.is_empty() && !password_env.is_empty();

    if auth_enabled {
        if is_authed {
            info!(
                username = ?username.as_deref().unwrap_or(""),
                "Authentication success"
            );
        } else {
            info!(
                username = ?username.as_deref().unwrap_or(""),
                "Authentication failure"
            );
        }
    } else {
        info!(
            username = ?username.as_deref().unwrap_or(""),
            "Authentication bypassed (auth disabled)"
        );
    }

    if !is_authed {
        let mut response = (
            axum::http::StatusCode::UNAUTHORIZED,
            [
                (header::WWW_AUTHENTICATE, "Basic realm=\"CalDAV Server\""),
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            ],
            "Unauthorized",
        ).into_response();
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

    let caldav_routes = Router::new()
        .route(
            "/cal/{db_id}",
            axum::routing::any(handle_path_calendar),
        )
        .route(
            "/cal/{db_id}/",
            axum::routing::any(handle_path_calendar),
        )
        .route(
            "/cal/{db_id}/{event_id}",
            axum::routing::any(handle_path_calendar_event),
        )
        .route(
            "/cal/{db_id}/{event_id}/",
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
        .route("/refresh", post(move |State(state): State<AppState>| async move {
            state.refresh_all().await;
            "refresh triggered"
        }))
        .route(
            "/cal.ics",
            get(move |State(state): State<AppState>| async move {
                let cache = state.cache.read().await;
                let mut all_pages: Vec<PageInfo> = Vec::new();
                let mut names: Vec<String> = Vec::new();
                for (db_id, pages) in cache.iter() {
                    all_pages.extend(pages.clone());
                    names.push(format!("Notion {}", &db_id[..8]));
                }
                let name = names.join(", ");
                let body = build_ics("all", &name, &all_pages);
                ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response()
            }),
        )
        // Webview: server-rendered FullCalendar page + its JSON CRUD API,
        // writing straight through to Notion (see notion_create/update/
        // delete_event on AppState). Same auth as everything else here.
        .route("/app", get(crate::webview::handle_webview_index))
        .route("/app/{db_id}", get(crate::webview::handle_webview_page))
        .route("/app/{db_id}/api/events", get(crate::webview::handle_list_events).post(crate::webview::handle_create_event))
        .route(
            "/app/{db_id}/api/events/{event_id}",
            axum::routing::patch(crate::webview::handle_update_event).delete(crate::webview::handle_delete_event),
        )
        .route_layer(axum::middleware::from_fn(auth_middleware));

    // Built as its own router (not chained onto the others before calling
    // .layer()) because Router::layer() wraps *every* route already
    // registered on that router, not just the one added right before it —
    // chaining this after /health and /webhook/notion-test made those force
    // a Keycloak login redirect too, which isn't what "/me forces login" was
    // supposed to mean.
    let me_route = Router::new()
        .route("/me", get(crate::auth::me))
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
}
