//! Notion data model: sync state and event types.

use std::collections::HashMap;
use std::sync::Arc;

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

// ─────────────────────────────────────────────
// NotionFsTree: cached Notion calendars/events
// ─────────────────────────────────────────────

/// Thread-safe cache of synced events keyed by a monotonically increasing
/// index. The first element is always the most recent sync snapshot.
#[derive(Debug, Clone)]
pub struct NotionFsTree(pub Arc<parking_lot::Mutex<Vec<(u64, HashMap<String, CalendarInfo>)>>>);

impl Default for NotionFsTree {
    fn default() -> Self {
        Self::new()
    }
}

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
