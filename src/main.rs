use std::{
    collections::HashMap,
    env,
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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Page info for ICS
#[derive(Debug, Clone, Serialize)]
struct PageInfo {
    id: String,
    title: String,
    start: String,
    end: Option<String>,
    url: String,
    last_edited: String,
}

// Shared app state
#[derive(Clone)]
struct AppState {
    client: Client,
    notion_token: String,
    database_ids: Vec<String>,
    data_source_ids: Vec<String>,
    date_property: String,
    cache: Arc<RwLock<HashMap<String, Vec<PageInfo>>>>,
}

// Notion API response types
#[derive(Debug, Deserialize)]
struct NotionQueryResponse {
    results: Vec<serde_json::Value>,
}

impl AppState {
    async fn refresh_db(&self, _db_id: &str, ds_id: &str) -> Result<Vec<PageInfo>, String> {
        let url = format!("https://api.notion.com/v1/data_sources/{}/query", ds_id);

        let body = serde_json::json!({
            "filter": {
                "property": self.date_property,
                "date": { "is_not_empty": true }
            },
            "sorts": [
                { "property": self.date_property, "direction": "descending" }
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

            // Find title property
            let title = props
                .as_object()
                .and_then(|o| o.values().find(|v| v.get("type").and_then(|t| t.as_str()) == Some("title")))
                .and_then(|t| {
                    t.as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|item| item.get("plain_text"))
                        .and_then(|t| t.as_str().map(|s| s.to_string()))
                })
                .unwrap_or("(untitled)".to_string())
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

    async fn refresh_all(&self) {
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
}

fn ics_dt(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let cleaned = value.replace(['-', ':'], "");
    if cleaned.contains('T') {
        let parts: Vec<&str> = cleaned.split('T').collect();
        let date = parts[0];
        let time = parts[1].split('+').next().unwrap_or(parts[1]);
        let time = time.split('Z').next().unwrap_or(time);
        format!("{}T{}Z", date, time)
    } else {
        format!(";VALUE=DATE:{}", cleaned)
    }
}

fn build_ics(db_id: &str, name: &str, pages: &[PageInfo]) -> String {
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

fn escape_ics(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace(';', "\\;")
     .replace(',', "\\,")
     .replace('\n', "\\n")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let notion_token = env::var("NOTION_TOKEN").expect("NOTION_TOKEN required");
    let database_ids = env::var("DATABASE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if database_ids.is_empty() {
        tracing::warn!("DATABASE_IDS env var empty; set comma-separated Notion database IDs");
    }

    let data_source_ids = env::var("DATA_SOURCE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if data_source_ids.is_empty() {
        tracing::warn!("DATA_SOURCE_IDS env var empty; set comma-separated Notion data source IDs");
    }

    let date_property = env::var("DATE_PROPERTY").unwrap_or_else(|_| "Date".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let state = AppState {
        client: Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
        notion_token,
        database_ids,
        data_source_ids,
        date_property,
        cache: Arc::new(RwLock::new(HashMap::new())),
    };

    // Initial refresh
    state.refresh_all().await;

    // Periodic refresh: every 5 minutes
    let state2 = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            state2.refresh_all().await;
        }
    });

    let app = Router::new()
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
            get(move |State(state): State<AppState>, Path(db_id): Path<String>| async move {
                let cache = state.cache.read().await;
                let pages = cache.get(&db_id).cloned().unwrap_or_default();

                let name = match state.client
                    .get(format!("https://api.notion.com/v1/databases/{}", db_id))
                    .header("Authorization", format!("Bearer {}", state.notion_token))
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
                };

                let body = build_ics(&db_id, &name, &pages);
                ([(header::CONTENT_TYPE, "text/calendar; charset=utf-8")], body).into_response()
            }),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("notion-ical-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
