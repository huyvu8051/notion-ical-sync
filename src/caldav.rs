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

pub fn build_propfind_calendar(db_id: &str, display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/cal/{db_id}/</D:href>
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
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        db_id = db_id,
        display_name = display_name
    )
}

pub fn build_propfind_calendar_with_events(db_id: &str, display_name: &str, pages: &[PageInfo]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/cal/"#);
    xml.push_str(db_id);
    xml.push_str(r#"/"</D:href>
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
        xml.push_str(&format!(
            r#"
  <D:response>
    <D:href>/cal/{db_id}/{clean_id}.ics</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>
        <D:getetag>"{etag}"</D:getetag>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#,
            db_id = db_id,
            clean_id = clean_id,
            etag = etag
        ));
    }

    xml.push_str("\n</D:multistatus>");
    xml
}

pub fn build_propfind_event(db_id: &str, event_id: &str, page: &PageInfo) -> String {
    let clean_id = event_id.replace(".ics", "");
    let etag = &page.last_edited;
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/cal/{db_id}/{clean_id}.ics</D:href>
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
        db_id = db_id,
        clean_id = clean_id,
        etag = etag
    )
}

pub fn build_report_response(db_id: &str, calendar_name: &str, pages: &[PageInfo]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#);

    for page in pages {
        let clean_id = page.id.replace("-", "");
        let etag = &page.last_edited;
        let ics_body = build_ics(db_id, calendar_name, std::slice::from_ref(page));
        xml.push_str(&format!(
            r#"
  <D:response>
    <D:href>/cal/{db_id}/{clean_id}.ics</D:href>
    <D:propstat>
      <D:prop>
        <D:getetag>"{etag}"</D:getetag>
        <C:calendar-data><![CDATA[{ics_body}]]></C:calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>"#,
            db_id = db_id,
            clean_id = clean_id,
            etag = etag,
            ics_body = ics_body
        ));
    }

    xml.push_str("\n</D:multistatus>");
    xml
}

pub async fn handle_calendar(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(db_id): Path<String>,
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
            build_propfind_calendar_with_events(&db_id, &name, &pages)
        } else {
            build_propfind_calendar(&db_id, &name)
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
        let body = build_report_response(&db_id, &name, &pages);
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        ).into_response();
    }

    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

pub async fn handle_calendar_event(
    method: axum::http::Method,
    State(state): State<AppState>,
    Path((db_id, event_id)): Path<(String, String)>,
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
            let body = build_propfind_event(&db_id, &event_id_clean, page);
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

pub fn create_app(state: AppState) -> Router {
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
        .route(
            "/cal/{db_id}",
            axum::routing::any(handle_calendar),
        )
        .route(
            "/cal/{db_id}/{event_id}",
            axum::routing::any(handle_calendar_event),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}
