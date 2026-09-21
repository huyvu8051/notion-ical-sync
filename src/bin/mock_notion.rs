use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const MOCK_DATABASE_ID: &str = "mock-db-1";
const MOCK_DATA_SOURCE_ID: &str = "mock-ds-1";
const MOCK_DATABASE_TITLE: &str = "Mock Test Calendar";
const MOCK_ACCESS_TOKEN: &str = "mock-notion-access-token";
const MOCK_WORKSPACE_ID: &str = "mock-workspace-1";
const MOCK_WORKSPACE_NAME: &str = "Mock Workspace";
const MOCK_BOT_ID: &str = "mock-bot-1";

#[derive(Clone)]
struct MockPage {
    id: String,
    title: String,
    start: String,
    end: Option<String>,
    archived: bool,
    location: Option<String>,
    notes: Option<String>,
    priority: Option<String>,
    busy: Option<bool>,
    reminder: Option<i64>,
    travel_time: Option<i64>,
    last_edited: String,
}

impl MockPage {
    fn to_notion_json(&self) -> Value {
        let mut properties = serde_json::Map::new();
        properties.insert(
            "Name".to_string(),
            json!({ "type": "title", "title": [{ "plain_text": self.title }] }),
        );
        properties.insert(
            "Date".to_string(),
            json!({ "type": "date", "date": { "start": self.start, "end": self.end } }),
        );
        if let Some(v) = &self.location {
            properties.insert(
                "Location".to_string(),
                json!({ "type": "rich_text", "rich_text": [{ "plain_text": v }] }),
            );
        }
        if let Some(v) = &self.notes {
            properties.insert(
                "Notes".to_string(),
                json!({ "type": "rich_text", "rich_text": [{ "plain_text": v }] }),
            );
        }
        if let Some(v) = &self.priority {
            properties.insert(
                "Priority".to_string(),
                json!({ "type": "select", "select": { "name": v } }),
            );
        }
        if let Some(v) = self.busy {
            properties.insert(
                "Busy".to_string(),
                json!({ "type": "checkbox", "checkbox": v }),
            );
        }
        if let Some(v) = self.reminder {
            properties.insert(
                "Reminder".to_string(),
                json!({ "type": "number", "number": v }),
            );
        }
        if let Some(v) = self.travel_time {
            properties.insert(
                "Travel time".to_string(),
                json!({ "type": "number", "number": v }),
            );
        }
        json!({
            "id": self.id,
            "last_edited_time": self.last_edited,
            "properties": properties,
        })
    }
}

fn schema_json() -> Value {
    json!({
        "properties": {
            "Name": { "type": "title" },
            "Date": { "type": "date" },
            "Location": { "type": "rich_text" },
            "Notes": { "type": "rich_text" },
            "Priority": { "type": "select" },
            "Busy": { "type": "checkbox" },
            "Reminder": { "type": "number" },
            "Travel time": { "type": "number" },
        }
    })
}

struct MockState {
    pages: Vec<MockPage>,
    next_id: u64,
}

fn seed_pages() -> Vec<MockPage> {
    vec![
        MockPage {
            id: "mock-page-edmonton".to_string(),
            title: "Edmonton event (report repro, UTC-6)".to_string(),
            start: "2026-09-17T18:00:00.000-06:00".to_string(),
            end: Some("2026-09-17T19:00:00.000-06:00".to_string()),
            archived: false,
            location: Some("Edmonton office".to_string()),
            notes: Some("Should show 18:00-19:00 local time on any CalDAV client".to_string()),
            priority: Some("High".to_string()),
            busy: Some(true),
            reminder: Some(15),
            travel_time: Some(10),
            last_edited: "2026-09-17T00:00:00.000Z".to_string(),
        },
        MockPage {
            id: "mock-page-vietnam".to_string(),
            title: "Vietnam event (UTC+7)".to_string(),
            start: "2026-09-21T09:00:00.000+07:00".to_string(),
            end: Some("2026-09-21T10:00:00.000+07:00".to_string()),
            archived: false,
            location: None,
            notes: None,
            priority: Some("Medium".to_string()),
            busy: Some(false),
            reminder: None,
            travel_time: None,
            last_edited: "2026-09-21T00:00:00.000Z".to_string(),
        },
        MockPage {
            id: "mock-page-utc".to_string(),
            title: "UTC event (control, no offset)".to_string(),
            start: "2026-09-25T12:00:00.000Z".to_string(),
            end: None,
            archived: false,
            location: None,
            notes: None,
            priority: None,
            busy: None,
            reminder: None,
            travel_time: None,
            last_edited: "2026-09-25T00:00:00.000Z".to_string(),
        },
    ]
}

type SharedState = Arc<Mutex<MockState>>;

async fn oauth_authorize(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let redirect_uri = params.get("redirect_uri").cloned().unwrap_or_default();
    let state_param = params.get("state").cloned().unwrap_or_default();
    let mut approve_url =
        url::Url::parse(&redirect_uri).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap());
    approve_url
        .query_pairs_mut()
        .append_pair("code", "mock-auth-code")
        .append_pair("state", &state_param);
    Html(format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Mock Notion — Authorize</title>
<style>body {{ font-family: -apple-system, sans-serif; max-width: 420px; margin: 4rem auto; text-align: center; }}
a {{ display: inline-block; margin-top: 1.5rem; padding: 0.75rem 1.5rem; background: #000; color: #fff; text-decoration: none; border-radius: 8px; }}</style>
</head><body>
<h2>Mock Notion</h2>
<p>"{MOCK_DATABASE_TITLE}" wants to connect NotionCal to your workspace.</p>
<a href="{approve_url}">Approve access</a>
</body></html>"#,
        approve_url = approve_url.as_str(),
    ))
}

async fn oauth_token() -> impl IntoResponse {
    Json(json!({
        "access_token": MOCK_ACCESS_TOKEN,
        "workspace_id": MOCK_WORKSPACE_ID,
        "workspace_name": MOCK_WORKSPACE_NAME,
        "bot_id": MOCK_BOT_ID,
    }))
}

async fn search() -> impl IntoResponse {
    Json(json!({
        "results": [{
            "id": MOCK_DATA_SOURCE_ID,
            "parent": { "database_id": MOCK_DATABASE_ID },
            "title": [{ "plain_text": MOCK_DATABASE_TITLE }],
            "icon": { "emoji": "🧪" },
            "properties": schema_json()["properties"],
        }]
    }))
}

async fn get_data_source(Path(_id): Path<String>) -> impl IntoResponse {
    Json(schema_json())
}

async fn query_data_source(
    State(state): State<SharedState>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let results: Vec<Value> = state
        .pages
        .iter()
        .filter(|p| !p.archived)
        .map(MockPage::to_notion_json)
        .collect();
    Json(json!({ "results": results }))
}

async fn get_database(Path(_id): Path<String>) -> impl IntoResponse {
    Json(json!({ "title": [{ "plain_text": MOCK_DATABASE_TITLE }] }))
}

fn prop_str(properties: &Value, name: &str, inner: &str) -> Option<String> {
    properties
        .get(name)?
        .get(inner)?
        .as_array()?
        .first()?
        .get("text")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

async fn create_page(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    let mut state = state.lock().await;
    state.next_id += 1;
    let id = format!("mock-page-created-{}", state.next_id);
    let properties = body.get("properties").cloned().unwrap_or(json!({}));
    let title = prop_str(&properties, "Name", "title").unwrap_or_else(|| "(untitled)".to_string());
    let date = properties.get("Date").and_then(|d| d.get("date"));
    let start = date
        .and_then(|d| d.get("start"))
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();
    let end = date
        .and_then(|d| d.get("end"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    state.pages.push(MockPage {
        id: id.clone(),
        title,
        start,
        end,
        archived: false,
        location: prop_str(&properties, "Location", "rich_text"),
        notes: prop_str(&properties, "Notes", "rich_text"),
        priority: properties
            .get("Priority")
            .and_then(|p| p.get("select"))
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string()),
        busy: properties.get("Busy").and_then(|b| b.get("checkbox")).and_then(|b| b.as_bool()),
        reminder: properties.get("Reminder").and_then(|r| r.get("number")).and_then(|n| n.as_i64()),
        travel_time: properties
            .get("Travel time")
            .and_then(|r| r.get("number"))
            .and_then(|n| n.as_i64()),
        last_edited: chrono::Utc::now().to_rfc3339(),
    });
    tracing::info!(page_id = %id, "mock-notion: created page");
    (axum::http::StatusCode::CREATED, Json(json!({ "id": id })))
}

async fn patch_page(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let Some(page) = state.pages.iter_mut().find(|p| p.id == id) else {
        return (axum::http::StatusCode::NOT_FOUND, Json(json!({ "error": "not found" })));
    };
    if body.get("in_trash").and_then(|v| v.as_bool()) == Some(true)
        || body.get("archived").and_then(|v| v.as_bool()) == Some(true)
    {
        page.archived = true;
        tracing::info!(page_id = %id, "mock-notion: archived page");
        return (axum::http::StatusCode::OK, Json(json!({ "id": id })));
    }
    if let Some(properties) = body.get("properties") {
        if let Some(t) = prop_str(properties, "Name", "title") {
            page.title = t;
        }
        if let Some(date) = properties.get("Date").and_then(|d| d.get("date")) {
            if let Some(s) = date.get("start").and_then(|v| v.as_str()) {
                page.start = s.to_string();
            }
            page.end = date.get("end").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
        if let Some(v) = prop_str(properties, "Location", "rich_text") {
            page.location = Some(v);
        }
        if let Some(v) = prop_str(properties, "Notes", "rich_text") {
            page.notes = Some(v);
        }
        if let Some(v) = properties
            .get("Priority")
            .and_then(|p| p.get("select"))
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str())
        {
            page.priority = Some(v.to_string());
        }
        if let Some(v) = properties.get("Busy").and_then(|b| b.get("checkbox")).and_then(|b| b.as_bool()) {
            page.busy = Some(v);
        }
        if let Some(v) = properties.get("Reminder").and_then(|r| r.get("number")).and_then(|n| n.as_i64()) {
            page.reminder = Some(v);
        }
        if let Some(v) = properties
            .get("Travel time")
            .and_then(|r| r.get("number"))
            .and_then(|n| n.as_i64())
        {
            page.travel_time = Some(v);
        }
    }
    page.last_edited = chrono::Utc::now().to_rfc3339();
    tracing::info!(page_id = %id, "mock-notion: updated page");
    (axum::http::StatusCode::OK, Json(json!({ "id": id })))
}

async fn status_page(State(state): State<SharedState>) -> impl IntoResponse {
    let state = state.lock().await;
    let rows: String = state
        .pages
        .iter()
        .map(|p| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                p.id,
                p.title,
                p.start,
                p.end.as_deref().unwrap_or("—"),
                if p.archived { "archived" } else { "active" },
            )
        })
        .collect();
    Html(format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Mock Notion status</title>
<style>body {{ font-family: -apple-system, sans-serif; margin: 2rem; }} table {{ border-collapse: collapse; }} td, th {{ border: 1px solid #ddd; padding: 0.4rem 0.8rem; text-align: left; }}</style>
</head><body>
<h2>Mock Notion — {MOCK_DATABASE_TITLE}</h2>
<p>database_id={MOCK_DATABASE_ID} data_source_id={MOCK_DATA_SOURCE_ID}</p>
<table><tr><th>id</th><th>title</th><th>start</th><th>end</th><th>status</th></tr>{rows}</table>
</body></html>"#
    ))
}

async fn root_redirect() -> impl IntoResponse {
    Redirect::to("/status")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let port: u16 = std::env::var("MOCK_NOTION_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let state: SharedState = Arc::new(Mutex::new(MockState {
        pages: seed_pages(),
        next_id: 0,
    }));

    let app = Router::new()
        .route("/", get(root_redirect))
        .route("/status", get(status_page))
        .route("/v1/oauth/authorize", get(oauth_authorize))
        .route("/v1/oauth/token", post(oauth_token))
        .route("/v1/search", post(search))
        .route("/v1/data_sources/{id}", get(get_data_source))
        .route("/v1/data_sources/{id}/query", post(query_data_source))
        .route("/v1/databases/{id}", get(get_database))
        .route("/v1/pages", post(create_page))
        .route("/v1/pages/{id}", patch(patch_page))
        .with_state(state);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    tracing::info!("mock-notion listening on {addr} — status page at http://localhost:{port}/status");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
