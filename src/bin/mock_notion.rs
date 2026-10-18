//! A stand-in Notion API for local development.
//!
//! It serves two databases on purpose. The English one matches the names the
//! app used to hardcode, so it passes no matter what. The German one is the
//! realistic case — a title column that is not "Name", fields in another
//! language, and two date columns where the one the user wants ("Frist") sorts
//! alphabetically *after* one they do not ("Archivierungsdatum"). Picking the
//! first date column by map order chose the wrong one; the mapping UI is what
//! makes the right one reachable.

use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const MOCK_ACCESS_TOKEN: &str = "mock-notion-access-token";
const MOCK_WORKSPACE_ID: &str = "mock-workspace-1";
const MOCK_WORKSPACE_NAME: &str = "Mock Workspace";
const MOCK_BOT_ID: &str = "mock-bot-1";

/// Property names of one mock database.
#[derive(Clone, Copy)]
struct Cols {
    title: &'static str,
    date: &'static str,
    /// A second date column, to prove the date picker is a real choice.
    alt_date: Option<&'static str>,
    location: &'static str,
    notes: &'static str,
    priority: &'static str,
    busy: &'static str,
    reminder: &'static str,
    travel: &'static str,
}

#[derive(Clone, Copy)]
struct MockDb {
    database_id: &'static str,
    data_source_id: &'static str,
    title: &'static str,
    icon: &'static str,
    cols: Cols,
}

const EN_DB: MockDb = MockDb {
    database_id: "mock-db-1",
    data_source_id: "mock-ds-1",
    title: "Mock Test Calendar",
    icon: "🧪",
    cols: Cols {
        title: "Name",
        date: "Date",
        alt_date: None,
        location: "Location",
        notes: "Notes",
        priority: "Priority",
        busy: "Busy",
        reminder: "Reminder",
        travel: "Travel time",
    },
};

const DE_DB: MockDb = MockDb {
    database_id: "mock-db-de",
    data_source_id: "mock-ds-de",
    title: "Aufgabenplanung",
    icon: "🇩🇪",
    cols: Cols {
        title: "Aufgabe",
        date: "Frist",
        alt_date: Some("Archivierungsdatum"),
        location: "Ort",
        notes: "Notizen",
        priority: "Priorität",
        busy: "Gebucht",
        reminder: "Erinnerung",
        travel: "Fahrzeit",
    },
};

const DATABASES: [MockDb; 2] = [EN_DB, DE_DB];

fn db_by_data_source(data_source_id: &str) -> MockDb {
    DATABASES
        .into_iter()
        .find(|db| db.data_source_id == data_source_id)
        .unwrap_or(EN_DB)
}

#[derive(Clone)]
struct MockPage {
    id: String,
    data_source_id: String,
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
        let cols = db_by_data_source(&self.data_source_id).cols;
        let mut properties = serde_json::Map::new();
        properties.insert(
            cols.title.to_string(),
            json!({ "type": "title", "title": [{ "plain_text": self.title }] }),
        );
        properties.insert(
            cols.date.to_string(),
            json!({ "type": "date", "date": { "start": self.start, "end": self.end } }),
        );
        // Present but always empty, so a calendar pointed at it syncs nothing —
        // the symptom 35% of production calendars were showing.
        if let Some(alt) = cols.alt_date {
            properties.insert(
                alt.to_string(),
                json!({ "type": "date", "date": Value::Null }),
            );
        }
        if let Some(v) = &self.location {
            properties.insert(
                cols.location.to_string(),
                json!({ "type": "rich_text", "rich_text": [{ "plain_text": v }] }),
            );
        }
        if let Some(v) = &self.notes {
            properties.insert(
                cols.notes.to_string(),
                json!({ "type": "rich_text", "rich_text": [{ "plain_text": v }] }),
            );
        }
        if let Some(v) = &self.priority {
            properties.insert(
                cols.priority.to_string(),
                json!({ "type": "select", "select": { "name": v } }),
            );
        }
        if let Some(v) = self.busy {
            properties.insert(
                cols.busy.to_string(),
                json!({ "type": "checkbox", "checkbox": v }),
            );
        }
        if let Some(v) = self.reminder {
            properties.insert(
                cols.reminder.to_string(),
                json!({ "type": "number", "number": v }),
            );
        }
        if let Some(v) = self.travel_time {
            properties.insert(
                cols.travel.to_string(),
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

fn schema_json(db: &MockDb) -> Value {
    let cols = db.cols;
    let mut properties = serde_json::Map::new();
    properties.insert(cols.title.to_string(), json!({ "type": "title" }));
    properties.insert(cols.date.to_string(), json!({ "type": "date" }));
    if let Some(alt) = cols.alt_date {
        properties.insert(alt.to_string(), json!({ "type": "date" }));
    }
    properties.insert(cols.location.to_string(), json!({ "type": "rich_text" }));
    properties.insert(cols.notes.to_string(), json!({ "type": "rich_text" }));
    properties.insert(cols.priority.to_string(), json!({ "type": "select" }));
    properties.insert(cols.busy.to_string(), json!({ "type": "checkbox" }));
    properties.insert(cols.reminder.to_string(), json!({ "type": "number" }));
    properties.insert(cols.travel.to_string(), json!({ "type": "number" }));
    json!({ "properties": properties })
}

struct MockState {
    pages: Vec<MockPage>,
    next_id: u64,
}

fn seed_pages() -> Vec<MockPage> {
    let mut pages = vec![
        MockPage {
            id: "mock-page-edmonton".to_string(),
            data_source_id: EN_DB.data_source_id.to_string(),
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
            data_source_id: EN_DB.data_source_id.to_string(),
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
            data_source_id: EN_DB.data_source_id.to_string(),
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
        MockPage {
            id: "mock-page-de-abgabe".to_string(),
            data_source_id: DE_DB.data_source_id.to_string(),
            title: "Projektabgabe".to_string(),
            start: "2026-10-20T14:00:00.000+02:00".to_string(),
            end: Some("2026-10-20T15:30:00.000+02:00".to_string()),
            archived: false,
            location: Some("Büro Berlin".to_string()),
            notes: Some("Nur sichtbar, wenn \"Frist\" als Datumsspalte gewählt ist".to_string()),
            priority: Some("High".to_string()),
            busy: Some(true),
            reminder: Some(30),
            travel_time: Some(20),
            last_edited: "2026-10-14T00:00:00.000Z".to_string(),
        },
        MockPage {
            id: "mock-page-de-vorlesung".to_string(),
            data_source_id: DE_DB.data_source_id.to_string(),
            title: "Vorlesung Statistik".to_string(),
            start: "2026-10-22T09:00:00.000+02:00".to_string(),
            end: Some("2026-10-22T10:30:00.000+02:00".to_string()),
            archived: false,
            location: None,
            notes: None,
            priority: Some("Low".to_string()),
            busy: Some(false),
            reminder: None,
            travel_time: None,
            last_edited: "2026-10-14T00:00:00.000Z".to_string(),
        },
    ];

    // Pad the German database past Notion's 100-row page limit, matching the
    // size a user reported (185 dated rows). Spread over consecutive days so
    // that a client reading only the first page keeps the newest dates and
    // appears to start partway through the calendar.
    let mut day = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).expect("valid seed date");
    for i in 0..183 {
        pages.push(MockPage {
            id: format!("mock-page-de-bulk-{i:03}"),
            data_source_id: DE_DB.data_source_id.to_string(),
            title: format!("Termin {:03}", i + 1),
            start: day.format("%Y-%m-%d").to_string(),
            end: None,
            archived: false,
            location: None,
            notes: None,
            priority: None,
            busy: None,
            reminder: None,
            travel_time: None,
            last_edited: "2026-10-14T00:00:00.000Z".to_string(),
        });
        day = day.succ_opt().expect("date stays in range");
    }

    pages
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
<p>NotionCal wants to connect to your workspace.</p>
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
    let results: Vec<Value> = DATABASES
        .iter()
        .map(|db| {
            json!({
                "id": db.data_source_id,
                "parent": { "database_id": db.database_id },
                "title": [{ "plain_text": db.title }],
                "icon": { "emoji": db.icon },
                "properties": schema_json(db)["properties"],
            })
        })
        .collect();
    Json(json!({ "results": results }))
}

async fn get_data_source(Path(id): Path<String>) -> impl IntoResponse {
    Json(schema_json(&db_by_data_source(&id)))
}

async fn query_data_source(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Honour the `{property, date: {is_not_empty: true}}` filter the app sends.
    // Without this the mock returns rows real Notion would withhold, which
    // hides exactly the bug this database exists to reproduce: pointing a
    // calendar at an always-empty date column must yield no events.
    let filter_property = body
        .get("filter")
        .and_then(|f| f.get("property"))
        .and_then(|p| p.as_str())
        .map(|s| s.to_string());
    let requires_non_empty = body
        .pointer("/filter/date/is_not_empty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let state = state.lock().await;
    let mut matching: Vec<Value> = state
        .pages
        .iter()
        .filter(|p| !p.archived && p.data_source_id == id)
        .map(MockPage::to_notion_json)
        .filter(|page| {
            let (Some(property), true) = (filter_property.as_deref(), requires_non_empty) else {
                return true;
            };
            page.pointer("/properties")
                .and_then(|props| props.get(property))
                .and_then(|prop| prop.get("date"))
                .is_some_and(|date| !date.is_null())
        })
        .collect();

    // Sort the way the app asks, so paging is stable and the first page really
    // is the newest rows — the detail that made the truncation look like
    // "my calendar starts in June" rather than "my calendar is cut off".
    if let Some(property) = filter_property.as_deref() {
        let descending = body
            .pointer("/sorts/0/direction")
            .and_then(|d| d.as_str())
            .unwrap_or("ascending")
            == "descending";
        matching.sort_by(|a, b| {
            let key = |page: &Value| {
                page.pointer(&format!("/properties/{property}/date/start"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            if descending {
                key(b).cmp(&key(a))
            } else {
                key(a).cmp(&key(b))
            }
        });
    }

    // Page exactly like Notion: at most `page_size` (default and max 100) rows,
    // with an opaque cursor. Returning everything at once would let a client
    // that ignores `has_more` look correct.
    let page_size = body
        .get("page_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .clamp(1, 100) as usize;
    let start = body
        .get("start_cursor")
        .and_then(|v| v.as_str())
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0);

    let total = matching.len();
    let end = (start + page_size).min(total);
    let results: Vec<Value> = matching[start.min(total)..end].to_vec();
    let has_more = end < total;
    let next_cursor = has_more.then(|| end.to_string());

    tracing::info!(
        data_source_id = %id,
        start,
        returned = results.len(),
        total,
        has_more,
        filter_property = filter_property.as_deref().unwrap_or("<none>"),
        "mock-notion: query"
    );
    Json(json!({
        "results": results,
        "has_more": has_more,
        "next_cursor": next_cursor,
    }))
}

async fn get_database(Path(id): Path<String>) -> impl IntoResponse {
    let db = DATABASES
        .into_iter()
        .find(|db| db.database_id == id)
        .unwrap_or(EN_DB);
    Json(json!({ "title": [{ "plain_text": db.title }] }))
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

fn prop_select(properties: &Value, name: &str) -> Option<String> {
    properties
        .get(name)?
        .get("select")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

fn prop_number(properties: &Value, name: &str) -> Option<i64> {
    properties.get(name)?.get("number")?.as_i64()
}

/// Reject a write that names a property this database does not have, exactly
/// as Notion does. This is what produced the production error
/// `"Name is not a property that exists."` — the mock used to accept it.
fn reject_unknown_properties(db: &MockDb, properties: &Value) -> Option<String> {
    let schema = schema_json(db);
    let known = schema.get("properties")?.as_object()?;
    let posted = properties.as_object()?;
    posted
        .keys()
        .find(|name| !known.contains_key(*name))
        .cloned()
}

async fn create_page(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let data_source_id = body
        .get("parent")
        .and_then(|p| p.get("data_source_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(EN_DB.data_source_id)
        .to_string();
    let db = db_by_data_source(&data_source_id);
    let cols = db.cols;
    let properties = body.get("properties").cloned().unwrap_or(json!({}));

    if let Some(unknown) = reject_unknown_properties(&db, &properties) {
        tracing::warn!(property = %unknown, "mock-notion: rejected create with unknown property");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "object": "error",
                "status": 400,
                "code": "validation_error",
                "message": format!("{unknown} is not a property that exists."),
            })),
        );
    }

    let mut state = state.lock().await;
    state.next_id += 1;
    let id = format!("mock-page-created-{}", state.next_id);
    let title =
        prop_str(&properties, cols.title, "title").unwrap_or_else(|| "(untitled)".to_string());
    let date = properties.get(cols.date).and_then(|d| d.get("date"));
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
        data_source_id,
        title,
        start,
        end,
        archived: false,
        location: prop_str(&properties, cols.location, "rich_text"),
        notes: prop_str(&properties, cols.notes, "rich_text"),
        priority: prop_select(&properties, cols.priority),
        busy: properties
            .get(cols.busy)
            .and_then(|b| b.get("checkbox"))
            .and_then(|b| b.as_bool()),
        reminder: prop_number(&properties, cols.reminder),
        travel_time: prop_number(&properties, cols.travel),
        last_edited: chrono::Utc::now().to_rfc3339(),
    });
    tracing::info!(page_id = %id, "mock-notion: created page");
    (axum::http::StatusCode::CREATED, Json(json!({ "id": id })))
}

/// Single-page read, used by the app's post-write refresh so it does not have
/// to re-query the whole data source.
async fn get_page(State(state): State<SharedState>, Path(id): Path<String>) -> impl IntoResponse {
    let state = state.lock().await;
    match state.pages.iter().find(|p| p.id == id) {
        Some(page) => {
            let mut v = page.to_notion_json();
            v["archived"] = json!(page.archived);
            (axum::http::StatusCode::OK, Json(v))
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"object":"error","status":404,"code":"object_not_found",
                        "message":format!("Could not find page with ID: {id}.")})),
        ),
    }
}

async fn patch_page(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let Some(page) = state.pages.iter_mut().find(|p| p.id == id) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "error": "not found" })),
        );
    };
    if body.get("in_trash").and_then(|v| v.as_bool()) == Some(true)
        || body.get("archived").and_then(|v| v.as_bool()) == Some(true)
    {
        page.archived = true;
        tracing::info!(page_id = %id, "mock-notion: archived page");
        return (axum::http::StatusCode::OK, Json(json!({ "id": id })));
    }

    let db = db_by_data_source(&page.data_source_id);
    let cols = db.cols;
    if let Some(properties) = body.get("properties") {
        if let Some(unknown) = reject_unknown_properties(&db, properties) {
            tracing::warn!(property = %unknown, "mock-notion: rejected update with unknown property");
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "object": "error",
                    "status": 400,
                    "code": "validation_error",
                    "message": format!("{unknown} is not a property that exists."),
                })),
            );
        }
        if let Some(t) = prop_str(properties, cols.title, "title") {
            page.title = t;
        }
        if let Some(date) = properties.get(cols.date).and_then(|d| d.get("date")) {
            if let Some(s) = date.get("start").and_then(|v| v.as_str()) {
                page.start = s.to_string();
            }
            page.end = date
                .get("end")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        if let Some(v) = prop_str(properties, cols.location, "rich_text") {
            page.location = Some(v);
        }
        if let Some(v) = prop_str(properties, cols.notes, "rich_text") {
            page.notes = Some(v);
        }
        if let Some(v) = prop_select(properties, cols.priority) {
            page.priority = Some(v);
        }
        if let Some(v) = properties
            .get(cols.busy)
            .and_then(|b| b.get("checkbox"))
            .and_then(|b| b.as_bool())
        {
            page.busy = Some(v);
        }
        if let Some(v) = prop_number(properties, cols.reminder) {
            page.reminder = Some(v);
        }
        if let Some(v) = prop_number(properties, cols.travel) {
            page.travel_time = Some(v);
        }
    }
    page.last_edited = chrono::Utc::now().to_rfc3339();
    tracing::info!(page_id = %id, "mock-notion: updated page");
    (axum::http::StatusCode::OK, Json(json!({ "id": id })))
}

async fn status_page(State(state): State<SharedState>) -> impl IntoResponse {
    let state = state.lock().await;
    let sections: String = DATABASES
        .iter()
        .map(|db| {
            let rows: String = state
                .pages
                .iter()
                .filter(|p| p.data_source_id == db.data_source_id)
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
            let cols = db.cols;
            format!(
                "<h2>{icon} {title}</h2><p>database_id={database_id} data_source_id={data_source_id}</p>\
                 <p>title column=<code>{title_col}</code>, date column=<code>{date_col}</code>{alt}</p>\
                 <table><tr><th>id</th><th>title</th><th>start</th><th>end</th><th>status</th></tr>{rows}</table>",
                icon = db.icon,
                title = db.title,
                database_id = db.database_id,
                data_source_id = db.data_source_id,
                title_col = cols.title,
                date_col = cols.date,
                alt = cols
                    .alt_date
                    .map(|a| format!(", decoy date column=<code>{a}</code> (always empty)"))
                    .unwrap_or_default(),
                rows = rows,
            )
        })
        .collect();
    Html(format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Mock Notion status</title>
<style>body {{ font-family: -apple-system, sans-serif; margin: 2rem; }} table {{ border-collapse: collapse; margin-bottom: 2rem; }} td, th {{ border: 1px solid #ddd; padding: 0.4rem 0.8rem; text-align: left; }} code {{ background: #f4f4f4; padding: 0.1rem 0.3rem; }}</style>
</head><body>
<h1>Mock Notion</h1>
{sections}
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
        .route("/v1/pages/{id}", get(get_page).patch(patch_page))
        .with_state(state);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    tracing::info!(
        "mock-notion listening on {addr} — status page at http://localhost:{port}/status"
    );
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
