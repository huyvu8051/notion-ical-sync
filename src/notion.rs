//! Notion data model: sync state and event types.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────
// Notion page (event) types
// ─────────────────────────────────────────────

/// A single synced Notion page, representing a calendar event.
#[derive(Debug, Clone, Serialize)]
pub struct NotionCalendarEvent {
    /// Notion UUID with dashes (e.g. "a1b2c3d4-e5f6-...")
    pub page_id_str: String,
    /// Human-readable title
    pub name: String,
    /// Start timestamp (unix seconds)
    pub start_timestamp: u64,
    /// End timestamp (unix seconds), or None for date-only events
    pub end_timestamp: Option<u64>,
    /// Description / URL text
    pub description: String,
    /// Full Notion URL
    pub notion_url: String,
    /// Notion property type: "date" or "date_range"
    pub property_type: String,
}

/// Metadata about a single Notion database (Calendar)
#[derive(Debug, Clone, Serialize)]
pub struct CalendarInfo {
    pub db_id: String,
    pub title: String,
    pub events: HashMap<String, NotionCalendarEvent>,
    pub last_synced: DateTime<Utc>,
}

impl CalendarInfo {
    pub fn new(db_id: String, title: String) -> Self {
        Self {
            db_id,
            title,
            events: HashMap::new(),
            last_synced: Utc::now(),
        }
    }
}

/// Calendar metadata pairing used internally
#[derive(Debug, Clone)]
pub struct CalendarMeta {
    pub db_id: String,
    pub data_source_id: String,
    pub title: String,
}

// ─────────────────────────────────────────────
// NotionFsTree: cached Notion calendars/events
// ─────────────────────────────────────────────

/// Thread-safe cache of synced events keyed by a monotonically increasing
/// index. The first element is always the most recent sync snapshot.
#[derive(Debug, Clone)]
pub struct NotionFsTree(pub Arc<parking_lot::Mutex<Vec<(u64, HashMap<String, CalendarInfo>)>>>);

impl NotionFsTree {
    pub fn new() -> Self {
        Self(Arc::new(parking_lot::Mutex::new(Vec::new())))
    }

    /// Push a new snapshot, returning its index.
    pub fn push(&self, map: HashMap<String, CalendarInfo>) -> u64 {
        let mut snap = self.0.lock();
        let idx = snap.len() as u64;
        snap.push((idx, map));
        idx
    }

    /// Update existing snapshot (replace last) – used for refresh.
    pub fn update_last(&self, map: HashMap<String, CalendarInfo>) {
        let mut snap = self.0.lock();
        if snap.is_empty() {
            snap.push((0, map));
        } else {
            let last = snap.len() - 1;
            snap[last] = (snap[last].0, map);
        }
    }
}

// ─────────────────────────────────────────────
// Notion Query Response types (relaxed)
// ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NotionQueryResponse {
    pub results: Vec<serde_json::Value>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NotionDbResponse {
    pub id: String,
    pub title: Vec<NotionTitleItem>,
}

#[derive(Debug, Deserialize)]
pub struct NotionTitleItem {
    pub plain_text: String,
}

// ─────────────────────────────────────────────
// Env config
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub notion_token: String,
    pub calendars: Vec<CalendarMeta>,
    pub date_property: String,
    pub refresh_secs: u64,
    pub listen_addr: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let notion_token = std::env::var("NOTION_TOKEN")
            .expect("NOTION_TOKEN environment variable required");

        // Read cal list in CSV format: db_id=ds_id pairs (one per line)
        // or via DATABASE_IDS + DATA_SOURCE_IDS comma-separated
        let database_ids: Vec<String> = std::env::var("DATABASE_IDS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let data_source_ids: Vec<String> = std::env::var("DATA_SOURCE_IDS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let date_property = std::env::var("DATE_PROPERTY")
            .unwrap_or_else(|_| "Date".to_string());

        let calendars: Vec<CalendarMeta> = database_ids
            .into_iter()
            .zip(data_source_ids)
            .map(|(db_id, ds_id)| CalendarMeta {
                db_id,
                data_source_id: ds_id,
                title: String::new(),
            })
            .collect();

        let refresh_secs = std::env::var("REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        let listen_addr = std::env::var("LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string());

        Self {
            notion_token,
            calendars,
            date_property,
            refresh_secs,
            listen_addr,
        }
    }
}
