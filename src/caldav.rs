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

// Shared app state
#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub notion_token: String,
    pub database_ids: Vec<String>,
    pub data_source_ids: Vec<String>,
    pub date_property: String,
    pub cache: Arc<RwLock<HashMap<String, Vec<PageInfo>>>>,
}

// Notion API response types
#[derive(Debug, Deserialize)]
struct NotionQueryResponse {
    results: Vec<serde_json::Value>,
}

impl AppState {
    pub fn new(
        notion_token: String,
        database_ids: Vec<String>,
        data_source_ids: Vec<String>,
        date_property: String,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            notion_token,
            database_ids,
            data_source_ids,
            date_property,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn refresh_db(&self, _db_id: &str, ds_id: &str) -> Result<Vec<PageInfo>, String> {
        let url = format!("https://api.notion.com/v1/data_sources/{}/query", ds_id);

        let body = serde_json::json!({
            "filter": {
                "property": &self.date_property,
                "date": { "is_not_empty": true }
            },
            "sorts": [
                { "property": &self.date_property, "direction": "descending" }
            ]
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.notion_token))
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

            let date_val = match props.get(&self.date_property).and_then(|v| v.get("date")) {
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
        let mut cache = self.cache.write().await;
        for (db_id, ds_id) in self.database_ids.iter().zip(&self.data_source_ids) {
            match self.refresh_db(db_id, ds_id).await {
                Ok(pages) => {
                    info!("DB {} synced: {} events", db_id, pages.len());
                    cache.insert(db_id.clone(), pages);
                }
                Err(e) => error!("DB {} refresh failed: {}", db_id, e),
            }
        }
    }

    pub async fn get_calendar_name(&self, db_id: &str) -> String {
        if self.notion_token == "mock-notion-token" {
            return format!("Notion {}", if db_id.len() >= 8 { &db_id[..8] } else { db_id });
        }
        match self.client
            .get(format!("https://api.notion.com/v1/databases/{}", db_id))
            .header("Authorization", format!("Bearer {}", self.notion_token))
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
        ics.push_str(&format!("DTSTART:{}\r\n", start));
        if !end.is_empty() {
            ics.push_str(&format!("DTEND:{}\r\n", end));
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

pub fn get_db_id_for_host(headers: &axum::http::HeaderMap, state: &AppState) -> Option<String> {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("").trim();
    let db_id = match host_name {
        "calendar.opendiy.vn" => Some("4cb38c7656ae483d8ee5650d9fb02108".to_string()),
        "mytime.opendiy.vn" => Some("39e6a94a90a680da85d2c29e3c52ed8e".to_string()),
        _ => None,
    };
    db_id.filter(|id| state.database_ids.contains(id))
}

pub async fn handle_calendar_impl(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    state: AppState,
    db_id: String,
    prefix: String,
) -> impl IntoResponse {
    let name = state.get_calendar_name(&db_id).await;

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
    let name = state.get_calendar_name(&db_id).await;
    let event_id_clean = event_id.strip_suffix(".ics").unwrap_or(&event_id).to_string();

    if method == axum::http::Method::GET {
        let cache = state.cache.read().await;
        let pages = cache.get(&db_id).cloned().unwrap_or_default();
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_ics(&db_id, &name, std::slice::from_ref(page));
            return ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response();
        } else {
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
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", db_id);
    let res = handle_calendar_impl(method, headers, state, db_id, prefix).await.into_response();
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
) -> impl IntoResponse {
    if let Some(db_id) = get_db_id_for_host(&headers, &state) {
        let prefix = "/".to_string();
        let res = handle_calendar_impl(method, headers, state, db_id, prefix).await.into_response();
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
    if let Some(db_id) = get_db_id_for_host(&headers, &state) {
        let prefix = "/".to_string();
        let res = handle_calendar_event_impl(method, state, db_id, event_id, prefix, body).await.into_response();
        add_caldav_headers(res)
    } else {
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
async fn handle_well_known() -> impl IntoResponse {
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
        let host_db_id = get_db_id_for_host(&headers, &state);
        let dbs_to_return = if let Some(db_id) = host_db_id {
            vec![db_id]
        } else {
            state.database_ids.clone()
        };

        let mut responses_xml = String::new();
        for db_id in dbs_to_return {
            let name = state.get_calendar_name(&db_id).await;
            let href = if get_db_id_for_host(&headers, &state).is_some() {
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
        let host_db_id = get_db_id_for_host(&headers, &state);
        let dbs_to_return = if let Some(db_id) = host_db_id {
            vec![db_id]
        } else {
            state.database_ids.clone()
        };

        let mut xml = String::new();
        xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#);

        let cache = state.cache.read().await;
        for db_id in dbs_to_return {
            let name = state.get_calendar_name(&db_id).await;
            let prefix = if get_db_id_for_host(&headers, &state).is_some() {
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

    if method == axum::http::Method::OPTIONS {
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

pub fn create_app(state: AppState) -> Router {
    let caldav_routes = Router::new()
        .route(
            "/cal/{db_id}",
            axum::routing::any(handle_path_calendar),
        )
        .route(
            "/cal/{db_id}/{event_id}",
            axum::routing::any(handle_path_calendar_event),
        )
        .route(
            "/.well-known/caldav",
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
        .route_layer(axum::middleware::from_fn(auth_middleware));

    Router::new()
        .route("/health", get(|| async { "ok" }))
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
        .layer(CorsLayer::permissive())
        .merge(caldav_routes)
        .with_state(state)
}

