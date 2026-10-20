use app::column_mapping::{ColumnMapping, Field};

use argon2::password_hash::PasswordVerifier;
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
use std::{collections::HashMap, time::Duration};
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PageInfo {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: Option<String>,
    pub url: String,
    pub last_edited: String,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<u8>,
    pub busy: Option<bool>,
    pub reminder_minutes: Option<i64>,
    pub travel_minutes: Option<i64>,
    pub repeat_rule: Option<String>,
    pub attendees: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct PageInfoRow {
    notion_page_id: String,
    title: String,
    start_at: String,
    end_at: Option<String>,
    url: String,
    last_edited: String,
    location: Option<String>,
    notes: Option<String>,
    priority: Option<i16>,
    busy: Option<bool>,
    reminder_minutes: Option<i64>,
    travel_minutes: Option<i64>,
    repeat_rule: Option<String>,
    attendees: Vec<String>,
}

impl From<PageInfoRow> for PageInfo {
    fn from(r: PageInfoRow) -> Self {
        PageInfo {
            id: r.notion_page_id,
            title: r.title,
            start: r.start_at,
            end: r.end_at,
            url: r.url,
            last_edited: r.last_edited,
            location: r.location,
            notes: r.notes,
            priority: r.priority.map(|p| p as u8),
            busy: r.busy,
            reminder_minutes: r.reminder_minutes,
            travel_minutes: r.travel_minutes,
            repeat_rule: r.repeat_rule,
            attendees: r.attendees,
        }
    }
}

fn find_property_case_insensitive<'a>(
    props: &'a serde_json::Value,
    name: &str,
) -> Option<&'a serde_json::Value> {
    props
        .as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

fn rich_text_plain(value: &serde_json::Value) -> Option<String> {
    let text: String = value
        .as_array()?
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(|t| t.as_str()))
        .collect();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_text_or_url_property(props: &serde_json::Value, name: &str) -> Option<String> {
    let prop = find_property_case_insensitive(props, name)?;
    match prop.get("type").and_then(|t| t.as_str())? {
        "rich_text" => rich_text_plain(prop.get("rich_text")?),
        "url" => prop.get("url")?.as_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn extract_select_name(props: &serde_json::Value, name: &str) -> Option<String> {
    let prop = find_property_case_insensitive(props, name)?;
    if prop.get("type").and_then(|t| t.as_str())? != "select" {
        return None;
    }
    prop.get("select")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

fn extract_number_property(props: &serde_json::Value, name: &str) -> Option<i64> {
    let prop = find_property_case_insensitive(props, name)?;
    if prop.get("type").and_then(|t| t.as_str())? != "number" {
        return None;
    }
    prop.get("number")?.as_f64().map(|n| n as i64)
}

fn map_priority(select_name: &str) -> Option<u8> {
    match select_name.to_lowercase().as_str() {
        "high" => Some(1),
        "medium" => Some(5),
        "low" => Some(9),
        _ => None,
    }
}

fn priority_number_to_select_name(n: u8) -> Option<&'static str> {
    match n {
        1..=3 => Some("High"),
        4..=6 => Some("Medium"),
        7..=9 => Some("Low"),
        _ => None,
    }
}

#[derive(Default)]
pub struct ExtraEventFields<'a> {
    pub location: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub priority: Option<u8>,
    pub busy: Option<bool>,
    pub reminder_minutes: Option<i64>,
    pub travel_minutes: Option<i64>,
}

/// Busy is the one field with two shapes: a `checkbox` that is directly the
/// answer, or a `select` ("Show As") whose value is compared against "busy".
/// `names` is tried in order, so an unconfigured calendar still gets the old
/// "Busy" then "Show As" behaviour.
fn extract_busy(props: &serde_json::Value, names: &[&str]) -> Option<bool> {
    for name in names {
        let Some(prop) = find_property_case_insensitive(props, name) else {
            continue;
        };
        match prop.get("type").and_then(|t| t.as_str()) {
            Some("checkbox") => {
                if let Some(b) = prop.get("checkbox").and_then(|v| v.as_bool()) {
                    return Some(b);
                }
            }
            Some("select") => {
                if let Some(value) = prop
                    .get("select")
                    .and_then(|s| s.get("name"))
                    .and_then(|n| n.as_str())
                {
                    return Some(value.eq_ignore_ascii_case("busy"));
                }
            }
            _ => {}
        }
    }
    None
}

fn map_repeat_to_rrule(select_name: &str) -> Option<String> {
    match select_name.to_lowercase().as_str() {
        "daily" => Some("FREQ=DAILY".to_string()),
        "weekly" => Some("FREQ=WEEKLY".to_string()),
        "monthly" => Some("FREQ=MONTHLY".to_string()),
        "yearly" => Some("FREQ=YEARLY".to_string()),
        _ => None,
    }
}

fn extract_attendees(props: &serde_json::Value, name: &str) -> Vec<String> {
    let Some(prop) = find_property_case_insensitive(props, name) else {
        return Vec::new();
    };
    match prop.get("type").and_then(|t| t.as_str()) {
        Some("people") => prop
            .get("people")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        p.get("person")
                            .and_then(|pp| pp.get("email"))
                            .and_then(|e| e.as_str())
                    })
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default(),
        Some("rich_text") => {
            rich_text_plain(prop.get("rich_text").unwrap_or(&serde_json::Value::Null))
                .map(|text| {
                    text.split(['\n', ',', ';'])
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum CaldavAllowWrites {
    #[default]
    False,
    True,
    Inbox,
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

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub db: PgPool,
    pub caldav_allow_writes: CaldavAllowWrites,
    pub webhook_secret: Option<String>,
    pub notion_oauth: Option<crate::pages::connect_notion::NotionOAuthConfig>,
    pub notion_api_base_url: String,
    pub mapbox_token: Option<String>,
    pub paddle: Option<crate::billing::PaddleConfig>,
    pub email: Option<crate::email::EmailConfig>,
    pub admin_secret: Option<String>,
    pub leptos_options: leptos::config::LeptosOptions,
}

impl axum::extract::FromRef<AppState> for leptos::config::LeptosOptions {
    fn from_ref(state: &AppState) -> Self {
        state.leptos_options.clone()
    }
}

#[derive(Debug, Deserialize)]
struct NotionQueryResponse {
    results: Vec<serde_json::Value>,
    /// Notion caps a query at 100 rows per page. Ignoring these two fields
    /// silently truncated every database with more than 100 dated rows — and
    /// because the query sorts by date descending, the rows that survived were
    /// the newest ones, so a calendar appeared to simply start partway through
    /// the year. Worse, `remove_stale_page_state` then deleted the older
    /// events from the cache on every pull, so they disappeared from clients
    /// that had already seen them.
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_cursor: Option<String>,
}

/// Upper bound on pages fetched per calendar (100 rows each), so a malformed
/// `next_cursor` that never advances cannot spin forever against the API.
const MAX_QUERY_PAGES: usize = 100;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CalendarRow {
    pub id: i64,
    pub user_id: i64,
    pub database_id: String,
    pub public_id: String,
    pub data_source_id: String,
    pub date_property: String,
    pub display_name: String,
    pub caldav_username: String,
    pub notion_access_token: String,
    pub notion_connection_id: i64,
    pub workspace_name: String,
    #[sqlx(json)]
    pub column_mapping: ColumnMapping,
    /// '' while Notion answers for this data source, else 'trashed' or 'gone'.
    pub source_state: String,
    pub source_checked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Column list shared by every `CalendarRow` query, so adding a field does not
/// mean hunting down five hand-copied SELECTs again.
const CALENDAR_ROW_COLUMNS: &str =
    "c.id, c.user_id, c.database_id, c.public_id, c.data_source_id, \
     c.date_property, c.display_name, c.caldav_username, c.column_mapping, \
     c.source_state, c.source_checked_at, \
     nc.notion_access_token, nc.id AS notion_connection_id, nc.workspace_name";

/// `refresh_db`'s failure modes. `Unauthorized` means Notion rejected the
/// bearer token itself (not a per-page permission issue — that's 403/404),
/// so the token is dead and retrying won't help; callers pause the whole
/// connection on this variant instead of just logging and moving on.
pub enum RefreshError {
    Unauthorized,
    /// Notion will not serve this data source any more: the user trashed or
    /// deleted the database. Distinct from `Other` because retrying is
    /// pointless and the owner is the only one who can fix it.
    SourceUnavailable(SourceState),
    Other(String),
}

/// Which flavour of "Notion no longer serves this data source".
///
/// The query endpoint cannot tell these apart — it answers 404 with the same
/// "make sure the relevant pages and databases are shared" text for both, and
/// for a genuine permission problem too. `GET /v1/data_sources/{id}` can:
/// a trashed source still returns 200, carrying `in_trash: true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// In Notion's trash. The user can restore it themselves.
    Trashed,
    /// Does not resolve at all — deleted for good, or no longer shared.
    Gone,
}

impl SourceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trashed => "trashed",
            Self::Gone => "gone",
        }
    }

    /// Wording aimed at the calendar's owner, shown on the sync log page.
    pub fn owner_message(self) -> &'static str {
        match self {
            Self::Trashed => {
                "This Notion database is in your trash, so Notion refuses to serve it. \
                 Restore it in Notion to resume syncing, or delete this calendar."
            }
            Self::Gone => {
                "This Notion database no longer exists, or is no longer shared with the \
                 NotionCal integration. Re-share it in Notion, or delete this calendar."
            }
        }
    }
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => {
                write!(f, "Notion error 401 Unauthorized: token revoked or invalid")
            }
            Self::SourceUnavailable(state) => write!(f, "{}", state.owner_message()),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedCaldavUser {
    pub user_id: i64,
    pub username: String,
    pub scoped_public_id: Option<String>,
}

/// Turn one Notion page object into a `PageInfo`, or `None` when it cannot
/// become an event — no properties, or the date property empty.
///
/// Shared by the full refresh and the single-page upsert so both interpret a
/// row identically; a second copy of this logic would drift from the first.
fn page_to_event(
    page: &serde_json::Value,
    date_property: &str,
    mapping: &ColumnMapping,
) -> Option<PageInfo> {

        let props = page.get("properties")?;

        let title = mapping
            .name_for(Field::Title)
            .and_then(|name| find_property_case_insensitive(props, name))
            .or_else(|| {
                props.as_object().and_then(|o| {
                    o.values()
                        .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("title"))
                })
            })
            .and_then(|t| t.get("title"))
            .and_then(|arr| arr.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("plain_text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)")
            .to_string();

        // The date column is matched case-insensitively like every other
        // field — an exact-case lookup here used to drop every page
        // whenever the stored name differed only in capitalisation.
        // An unset date comes back as `"date": null`, which is `Some(Null)`
        // rather than `None` — without the null check such a page becomes
        // an event with an empty start. The Notion query already filters
        // on `is_not_empty`, but relying on that alone means one filter
        // change silently fills the cache with dateless rows.
        let date_val = match find_property_case_insensitive(props, date_property)
            .and_then(|v| v.get("date"))
            .filter(|d| !d.is_null())
        {
            Some(d) => d,
            None => {
                return None;
            }
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

        let id = page
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let last_edited = page
            .get("last_edited_time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let notion_url = format!("https://notion.so/{}", id.replace("-", ""));

        let by_name = |field: Field| mapping.name_for(field);
        let location =
            by_name(Field::Location).and_then(|name| extract_text_or_url_property(props, name));
        let notes =
            by_name(Field::Notes).and_then(|name| extract_text_or_url_property(props, name));
        let priority = by_name(Field::Priority)
            .and_then(|name| extract_select_name(props, name))
            .and_then(|s| map_priority(&s));
        let busy = extract_busy(props, &mapping.names_for(Field::Busy));
        let reminder_minutes =
            by_name(Field::Reminder).and_then(|name| extract_number_property(props, name));
        let travel_minutes =
            by_name(Field::TravelMinutes).and_then(|name| extract_number_property(props, name));
        let repeat_rule = by_name(Field::Repeat)
            .and_then(|name| extract_select_name(props, name))
            .and_then(|s| map_repeat_to_rrule(&s));
        let attendees = by_name(Field::Attendees)
            .map(|name| extract_attendees(props, name))
            .unwrap_or_default();

        Some(PageInfo {
            id,
            title,
            start,
            end,
            url: notion_url,
            last_edited,
            location,
            notes,
            priority,
            busy,
            reminder_minutes,
            travel_minutes,
            repeat_rule,
        attendees,
    })

}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: PgPool,
        caldav_allow_writes: CaldavAllowWrites,
        webhook_secret: Option<String>,
        notion_oauth: Option<crate::pages::connect_notion::NotionOAuthConfig>,
        notion_api_base_url: String,
        mapbox_token: Option<String>,
        paddle: Option<crate::billing::PaddleConfig>,
        email: Option<crate::email::EmailConfig>,
        admin_secret: Option<String>,
        leptos_options: leptos::config::LeptosOptions,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            db,
            caldav_allow_writes,
            webhook_secret,
            notion_oauth,
            notion_api_base_url,
            mapbox_token,
            paddle,
            email,
            admin_secret,
            leptos_options,
        }
    }

    pub async fn all_calendars(&self) -> Vec<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(&format!(
            "SELECT {CALENDAR_ROW_COLUMNS}
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE nc.token_invalid_since IS NULL",
        ))
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to list calendars from db: {}", e);
            Vec::new()
        })
    }

    pub async fn calendars_for_user(&self, user_id: i64) -> Vec<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(&format!(
            "SELECT {CALENDAR_ROW_COLUMNS}
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.user_id = $1",
        ))
        .bind(user_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to list calendars for user {}: {}", user_id, e);
            Vec::new()
        })
    }

    pub async fn calendar_by_public_id(&self, public_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(&format!(
            "SELECT {CALENDAR_ROW_COLUMNS}
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.public_id = $1",
        ))
        .bind(public_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar {}: {}", public_id, e);
            None
        })
    }

    pub async fn lookup_caldav_uid(&self, calendar_id: i64, caldav_uid: &str) -> Option<String> {
        sqlx::query_scalar("SELECT notion_page_id FROM caldav_event_ids WHERE calendar_id = $1 AND caldav_uid = $2")
            .bind(calendar_id)
            .bind(caldav_uid)
            .fetch_optional(&self.db)
            .await
            .unwrap_or_else(|e| {
                error!("failed to look up caldav_uid mapping ({}, {}): {}", calendar_id, caldav_uid, e);
                None
            })
    }

    pub async fn store_caldav_uid_mapping(
        &self,
        calendar_id: i64,
        caldav_uid: &str,
        notion_page_id: &str,
    ) {
        if let Err(e) = sqlx::query(
            "INSERT INTO caldav_event_ids (calendar_id, caldav_uid, notion_page_id) VALUES ($1, $2, $3)
             ON CONFLICT (calendar_id, caldav_uid) DO UPDATE SET notion_page_id = EXCLUDED.notion_page_id",
        )
        .bind(calendar_id)
        .bind(caldav_uid)
        .bind(notion_page_id)
        .execute(&self.db)
        .await
        {
            error!("failed to store caldav_uid mapping ({}, {}) -> {}: {}", calendar_id, caldav_uid, notion_page_id, e);
        }
    }

    pub async fn delete_caldav_uid_mapping(&self, calendar_id: i64, caldav_uid: &str) {
        if let Err(e) =
            sqlx::query("DELETE FROM caldav_event_ids WHERE calendar_id = $1 AND caldav_uid = $2")
                .bind(calendar_id)
                .bind(caldav_uid)
                .execute(&self.db)
                .await
        {
            error!(
                "failed to delete caldav_uid mapping ({}, {}): {}",
                calendar_id, caldav_uid, e
            );
        }
    }

    async fn log_diff(&self, calendar_id: i64, new_pages: &[PageInfo]) {
        let calendar_already_tracked = self.calendar_page_state_exists(calendar_id).await;

        for page in new_pages {
            let upsert_result = self.upsert_page_state(calendar_id, page).await;
            if !calendar_already_tracked {
                continue;
            }
            match upsert_result {
                Some(true) => {
                    self.log_sync(
                        calendar_id,
                        "notion",
                        "create",
                        &page.id,
                        &page.id,
                        "ok",
                        "",
                    )
                    .await;
                }
                Some(false) => {
                    self.log_sync(
                        calendar_id,
                        "notion",
                        "update",
                        &page.id,
                        &page.id,
                        "ok",
                        "",
                    )
                    .await;
                }
                None => {}
            }
        }

        if !calendar_already_tracked {
            return;
        }

        for page_id in self.remove_stale_page_state(calendar_id, new_pages).await {
            self.log_sync(
                calendar_id,
                "notion",
                "delete",
                &page_id,
                &page_id,
                "ok",
                "",
            )
            .await;
        }
    }

    async fn calendar_page_state_exists(&self, calendar_id: i64) -> bool {
        let (count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM calendar_page_state WHERE calendar_id = $1")
                .bind(calendar_id)
                .fetch_one(&self.db)
                .await
                .unwrap_or((0,));
        count > 0
    }

    async fn upsert_page_state(&self, calendar_id: i64, page: &PageInfo) -> Option<bool> {
        sqlx::query_as(
            "INSERT INTO calendar_page_state
                (calendar_id, notion_page_id, last_edited, title, start_at, end_at, url,
                 location, notes, priority, busy, reminder_minutes, travel_minutes,
                 repeat_rule, attendees)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (calendar_id, notion_page_id)
             DO UPDATE SET
                last_edited = EXCLUDED.last_edited,
                title = EXCLUDED.title,
                start_at = EXCLUDED.start_at,
                end_at = EXCLUDED.end_at,
                url = EXCLUDED.url,
                location = EXCLUDED.location,
                notes = EXCLUDED.notes,
                priority = EXCLUDED.priority,
                busy = EXCLUDED.busy,
                reminder_minutes = EXCLUDED.reminder_minutes,
                travel_minutes = EXCLUDED.travel_minutes,
                repeat_rule = EXCLUDED.repeat_rule,
                attendees = EXCLUDED.attendees
             WHERE calendar_page_state.last_edited IS DISTINCT FROM EXCLUDED.last_edited
             RETURNING (xmax = 0)",
        )
        .bind(calendar_id)
        .bind(&page.id)
        .bind(&page.last_edited)
        .bind(&page.title)
        .bind(&page.start)
        .bind(&page.end)
        .bind(&page.url)
        .bind(&page.location)
        .bind(&page.notes)
        .bind(page.priority.map(|p| p as i16))
        .bind(page.busy)
        .bind(page.reminder_minutes)
        .bind(page.travel_minutes)
        .bind(&page.repeat_rule)
        .bind(&page.attendees)
        .fetch_optional(&self.db)
        .await
        .unwrap_or(None)
        .map(|(inserted,)| inserted)
    }

    pub async fn pages_for_calendar(&self, calendar_id: i64) -> Vec<PageInfo> {
        sqlx::query_as::<_, PageInfoRow>(
            "SELECT cps.notion_page_id, cps.title, cps.start_at, cps.end_at, cps.url, cps.last_edited,
                    cps.location, cps.notes, cps.priority, cps.busy, cps.reminder_minutes, cps.travel_minutes,
                    cps.repeat_rule, cps.attendees
             FROM calendar_page_state cps
             JOIN calendars synced_calendar ON synced_calendar.id = cps.calendar_id
             JOIN calendars requested_calendar ON requested_calendar.database_id = synced_calendar.database_id
             WHERE requested_calendar.id = $1
             ORDER BY cps.start_at DESC",
        )
        .bind(calendar_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to load cached pages for calendar {}: {}", calendar_id, e);
            Vec::new()
        })
        .into_iter()
        .map(PageInfo::from)
        .collect()
    }

    async fn remove_stale_page_state(
        &self,
        calendar_id: i64,
        current_pages: &[PageInfo],
    ) -> Vec<String> {
        let current_ids: Vec<String> = current_pages.iter().map(|p| p.id.clone()).collect();
        sqlx::query_scalar(
            "DELETE FROM calendar_page_state
             WHERE calendar_id = $1 AND NOT (notion_page_id = ANY($2))
             RETURNING notion_page_id",
        )
        .bind(calendar_id)
        .bind(&current_ids)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_sync(
        &self,
        calendar_id: i64,
        source: &str,
        action: &str,
        event_uid: &str,
        notion_page_id: &str,
        status: &str,
        detail: &str,
    ) {
        if let Err(e) = sqlx::query(
            "INSERT INTO sync_log (calendar_id, source, action, event_uid, notion_page_id, status, detail)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(calendar_id)
        .bind(source)
        .bind(action)
        .bind(event_uid)
        .bind(notion_page_id)
        .bind(status)
        .bind(detail)
        .execute(&self.db)
        .await
        {
            error!("failed to write sync_log row for calendar {}: {}", calendar_id, e);
        }
    }

    async fn calendar_by_db_id(&self, db_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(&format!(
            "SELECT {CALENDAR_ROW_COLUMNS}
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.database_id = $1
             ORDER BY c.created_at ASC
             LIMIT 1",
        ))
        .bind(db_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar {}: {}", db_id, e);
            None
        })
    }

    pub async fn calendar_by_data_source_id(&self, ds_id: &str) -> Option<CalendarRow> {
        sqlx::query_as::<_, CalendarRow>(&format!(
            "SELECT {CALENDAR_ROW_COLUMNS}
             FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id
             WHERE c.data_source_id = $1 AND nc.token_invalid_since IS NULL",
        ))
        .bind(ds_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("failed to look up calendar by data source {}: {}", ds_id, e);
            None
        })
    }

    pub async fn verify_caldav_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Option<(i64, Option<String>)> {
        #[derive(sqlx::FromRow)]
        struct CalendarCredRow {
            user_id: i64,
            public_id: String,
            caldav_password_hash: String,
        }

        if let Some(row) = sqlx::query_as::<_, CalendarCredRow>(
            "SELECT user_id, public_id, caldav_password_hash FROM calendars WHERE caldav_username = $1",
        )
        .bind(username)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("caldav credential lookup failed: {}", e);
            None
        }) {
            let hash = argon2::PasswordHash::new(&row.caldav_password_hash).ok()?;
            argon2::Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .ok()?;
            return Some((row.user_id, Some(row.public_id)));
        }

        #[derive(sqlx::FromRow)]
        struct AccountCredRow {
            id: i64,
            account_caldav_password_hash: String,
        }

        let row: AccountCredRow = sqlx::query_as::<_, AccountCredRow>(
            "SELECT id, account_caldav_password_hash FROM users WHERE account_caldav_username = $1",
        )
        .bind(username)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!("account caldav credential lookup failed: {}", e);
            None
        })?;

        let hash = argon2::PasswordHash::new(&row.account_caldav_password_hash).ok()?;
        argon2::Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .ok()?;
        Some((row.id, None))
    }

    pub async fn refresh_for_user(&self, user_id: i64) {
        for cal in self.calendars_for_user(user_id).await {
            match self
                .refresh_db(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    &cal.column_mapping,
                )
                .await
            {
                Ok(pages) => {
                    info!(
                        connection_id = cal.notion_connection_id,
                        workspace = %cal.workspace_name,
                        calendar = %cal.display_name,
                        database_id = %cal.database_id,
                        events = pages.len(),
                        "pull succeeded"
                    );
                    self.log_diff(cal.id, &pages).await;
                    self.clear_pull_failures(cal.id).await;
                    self.note_source_available(&cal).await;
                }
                Err(RefreshError::Unauthorized) => {
                    error!(
                        connection_id = cal.notion_connection_id,
                        workspace = %cal.workspace_name,
                        calendar = %cal.display_name,
                        database_id = %cal.database_id,
                        "pull failed: unauthorized (token revoked?)"
                    );
                    self.mark_notion_connection_invalid_and_notify(cal.notion_connection_id)
                        .await;
                }
                Err(e) => {
                    if let RefreshError::SourceUnavailable(state) = e {
                        self.note_source_unavailable(&cal, state).await;
                    }
                    self.log_pull_failure(&cal, &e).await
                }
            }
        }
    }

    /// Clear the failure marker after a pull succeeds.
    ///
    /// A successful pull writes no row of its own (only actual event changes
    /// do), so without this a one-off failure would keep the "this calendar is
    /// not syncing" banner up forever. Dropping the rows keeps "a pull error
    /// row exists" equivalent to "this calendar is failing right now", which
    /// is the question the banner answers.
    async fn clear_pull_failures(&self, calendar_id: i64) {
        if let Err(e) = sqlx::query(
            "DELETE FROM sync_log WHERE calendar_id = $1 AND source = 'notion' \
             AND action = 'pull' AND status = 'error'",
        )
        .bind(calendar_id)
        .execute(&self.db)
        .await
        {
            error!(calendar_id, error = %e, "failed to clear pull failure rows");
        }
    }

    /// Remember that Notion has stopped serving this calendar's data source.
    ///
    /// Only the first detection logs at warn level: the steady state is noisy
    /// (production had 19 such calendars, each retried hourly), and an alert
    /// wants the moment a calendar breaks, not a count of ones already broken.
    async fn note_source_unavailable(&self, cal: &CalendarRow, state: SourceState) {
        let changed = cal.source_state != state.as_str();
        if changed {
            warn!(
                calendar_id = cal.id,
                connection_id = cal.notion_connection_id,
                workspace = %cal.workspace_name,
                calendar = %cal.display_name,
                database_id = %cal.database_id,
                state = state.as_str(),
                "notion source became unavailable"
            );
        }
        if let Err(e) = sqlx::query(
            "UPDATE calendars SET source_state = $2, source_checked_at = now() WHERE id = $1",
        )
        .bind(cal.id)
        .bind(state.as_str())
        .execute(&self.db)
        .await
        {
            error!(calendar_id = cal.id, error = %e, "failed to record source state");
        }
    }

    /// Clear the marker once Notion serves the data source again — a user who
    /// restores a database from the trash should not have to reconnect.
    async fn note_source_available(&self, cal: &CalendarRow) {
        if cal.source_state.is_empty() {
            return;
        }
        info!(
            calendar_id = cal.id,
            connection_id = cal.notion_connection_id,
            calendar = %cal.display_name,
            was = %cal.source_state,
            "notion source recovered"
        );
        if let Err(e) = sqlx::query(
            "UPDATE calendars SET source_state = '', source_checked_at = now() WHERE id = $1",
        )
        .bind(cal.id)
        .execute(&self.db)
        .await
        {
            error!(calendar_id = cal.id, error = %e, "failed to clear source state");
        }
    }

    /// How long to leave a known-dead calendar alone before re-probing.
    ///
    /// Without this the sync loop spends six requests an hour per broken
    /// calendar discovering the same deletion; with 19 broken on production
    /// that was ~2,700 wasted Notion calls a day. Hourly still picks up a
    /// restore quickly enough that a user who undeletes a database sees their
    /// calendar come back within the hour.
    fn source_probe_is_due(cal: &CalendarRow) -> bool {
        if cal.source_state.is_empty() {
            return true;
        }
        match cal.source_checked_at {
            Some(at) => chrono::Utc::now() - at >= chrono::Duration::hours(1),
            None => true,
        }
    }

    /// Record a failed pull where the owner can see it.
    ///
    /// Pull failures used to go only to `tracing`, so a user whose calendar had
    /// silently stopped syncing had no way to find out — the sync log showed
    /// only successful writes. Now the same row that drives the sync log page
    /// carries the Notion error text.
    async fn log_pull_failure(&self, cal: &CalendarRow, err: &RefreshError) {
        error!(
            connection_id = cal.notion_connection_id,
            workspace = %cal.workspace_name,
            calendar = %cal.display_name,
            database_id = %cal.database_id,
            date_property = %cal.date_property,
            error = %err,
            "pull failed"
        );
        self.log_sync(cal.id, "notion", "pull", "", "", "error", &err.to_string())
            .await;
    }

    /// Ask Notion whether a data source that failed to answer a query is
    /// trashed or gone outright.
    ///
    /// `GET /v1/data_sources/{id}` still returns 200 for a trashed source, with
    /// `in_trash: true`; a deleted or unshared one 404s. That difference is the
    /// only way to tell the two apart, and it matters because a trashed
    /// database is something the owner can restore themselves.
    ///
    /// Falls back to `Gone` when the probe itself fails: the calendar is not
    /// syncing either way, and claiming "restore it from the trash" when we do
    /// not know that would send the owner looking for something that is not
    /// there.
    async fn probe_source_state(&self, ds_id: &str, notion_token: &str) -> SourceState {
        let url = format!("{}/v1/data_sources/{}", self.notion_api_base_url, ds_id);
        let resp = match self
            .client
            .get(&url)
            .bearer_auth(notion_token)
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                error!(notion_url = %url, error = %e, "source state probe failed (transport)");
                return SourceState::Gone;
            }
        };
        if !resp.status().is_success() {
            return SourceState::Gone;
        }
        let in_trash = resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("in_trash").and_then(|t| t.as_bool()))
            .unwrap_or(false);
        if in_trash {
            SourceState::Trashed
        } else {
            // Readable but unqueryable, which no production case produced.
            // Treat it as gone so it is still surfaced rather than retried.
            SourceState::Gone
        }
    }

    pub async fn refresh_db(
        &self,
        ds_id: &str,
        date_property: &str,
        notion_token: &str,
        mapping: &ColumnMapping,
    ) -> Result<Vec<PageInfo>, RefreshError> {
        let url = format!(
            "{}/v1/data_sources/{}/query",
            self.notion_api_base_url, ds_id
        );

        // Walk every page of results, not just the first. A database with more
        // than 100 dated rows otherwise loses everything past the first page.
        let mut rows: Vec<serde_json::Value> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages_fetched = 0usize;

        loop {
            let mut body = serde_json::json!({
                "filter": {
                    "property": date_property,
                    "date": { "is_not_empty": true }
                },
                "sorts": [
                    { "property": date_property, "direction": "descending" }
                ],
                "page_size": 100,
            });
            if let Some(cursor) = &cursor {
                body["start_cursor"] = serde_json::json!(cursor);
            }

            info!(notion_method = "POST", notion_url = %url, page = pages_fetched + 1, "-> Notion API request");
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
                    RefreshError::Other(format!("Request failed: {}", e))
                })?;

            let resp_status = resp.status();
            if !resp_status.is_success() {
                let txt = resp.text().await.unwrap_or_default();
                error!(notion_url = %url, status = %resp_status, body = %txt, "<- Notion API error response");
                if resp_status == reqwest::StatusCode::UNAUTHORIZED {
                    return Err(RefreshError::Unauthorized);
                }
                // A 404 here reads like a permission problem, but on production
                // every single one turned out to be a database the user had
                // trashed or deleted. Ask Notion which it is rather than
                // retrying the same dead query every ten minutes forever.
                if resp_status == reqwest::StatusCode::NOT_FOUND
                    && txt.contains("object_not_found")
                {
                    let state = self.probe_source_state(ds_id, notion_token).await;
                    return Err(RefreshError::SourceUnavailable(state));
                }
                return Err(RefreshError::Other(format!(
                    "Notion error {}: {}",
                    resp_status, txt
                )));
            }

            let data: NotionQueryResponse = resp
                .json()
                .await
                .map_err(|e| RefreshError::Other(format!("Parse failed: {}", e)))?;
            pages_fetched += 1;
            info!(
                notion_url = %url,
                status = %resp_status,
                page = pages_fetched,
                page_count = data.results.len(),
                has_more = data.has_more,
                "<- Notion API response"
            );
            rows.extend(data.results);

            match data.next_cursor {
                Some(next) if data.has_more => cursor = Some(next),
                _ => break,
            }

            if pages_fetched >= MAX_QUERY_PAGES {
                warn!(
                    notion_url = %url,
                    pages_fetched,
                    rows = rows.len(),
                    "stopping pagination at the page cap; some rows may be missing"
                );
                break;
            }
        }

        info!(
            notion_url = %url,
            pages_fetched,
            total_rows = rows.len(),
            "<- Notion query complete"
        );

        info!(
            notion_url = %url,
            date_property,
            title = mapping.name_for(Field::Title).unwrap_or("<none>"),
            location = mapping.name_for(Field::Location).unwrap_or("<none>"),
            notes = mapping.name_for(Field::Notes).unwrap_or("<none>"),
            priority = mapping.name_for(Field::Priority).unwrap_or("<none>"),
            busy = mapping.name_for(Field::Busy).unwrap_or("<none>"),
            reminder = mapping.name_for(Field::Reminder).unwrap_or("<none>"),
            travel_minutes = mapping.name_for(Field::TravelMinutes).unwrap_or("<none>"),
            repeat = mapping.name_for(Field::Repeat).unwrap_or("<none>"),
            attendees = mapping.name_for(Field::Attendees).unwrap_or("<none>"),
            configured = mapping.configured,
            "reading pages with column mapping"
        );

        let mut events = Vec::new();
        let mut skipped_no_date = 0usize;
        for page in rows {
            match page_to_event(&page, date_property, mapping) {
                Some(ev) => events.push(ev),
                None => skipped_no_date += 1,
            }
        }

        if skipped_no_date > 0 {
            info!(
                notion_url = %url,
                date_property,
                skipped_no_date,
                kept = events.len(),
                "pages skipped: date property missing on the page"
            );
        }

        Ok(events)
    }

    /// Pauses this connection (future refreshes skip it, see `all_calendars`/
    /// `calendar_by_data_source_id`'s `token_invalid_since IS NULL` filter)
    /// and emails the owner once. A no-op if already paused, so repeated
    /// 401s from the same dead token across sync cycles cost nothing extra.
    pub async fn mark_notion_connection_invalid_and_notify(&self, connection_id: i64) {
        let row: Option<(i64, String, String, String)> = sqlx::query_as(
            "UPDATE notion_connections nc SET token_invalid_since = now()
             FROM users u
             WHERE nc.id = $1 AND nc.token_invalid_since IS NULL AND u.id = nc.user_id
             RETURNING nc.id, nc.action_token, nc.workspace_name, u.email",
        )
        .bind(connection_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or_else(|e| {
            error!(
                "failed to mark notion_connection {} invalid: {}",
                connection_id, e
            );
            None
        });

        let Some((_, action_token, workspace_name, email)) = row else {
            return;
        };
        if email.is_empty() {
            return;
        }
        let Some(cfg) = self.email.clone() else {
            return;
        };

        let calendar_names: Vec<String> = sqlx::query_scalar(
            "SELECT display_name FROM calendars WHERE notion_connection_id = $1 ORDER BY display_name",
        )
        .bind(connection_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        let reconnect_url = "https://notion-caldav.opendiy.vn/connect/notion/start".to_string();
        let stop_url =
            format!("https://notion-caldav.opendiy.vn/notion-connection/{action_token}/stop");
        let (subject, html) = crate::email::notion_token_invalid_email(
            &workspace_name,
            &calendar_names,
            &reconnect_url,
            &stop_url,
        );
        crate::email::spawn_send(cfg, email, subject.to_string(), html);
    }

    pub async fn refresh_all(&self) {
        let calendars = self.all_calendars().await;
        info!(
            calendar_count = calendars.len(),
            "refresh_all: pull cycle started"
        );
        let mut seen = std::collections::HashSet::new();
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for cal in calendars {
            if !seen.insert(cal.database_id.clone()) {
                continue;
            }
            // A database the user trashed or deleted answers the same way every
            // time; re-asking six times an hour only burns rate limit.
            if !Self::source_probe_is_due(&cal) {
                skipped += 1;
                continue;
            }
            match self
                .refresh_db(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    &cal.column_mapping,
                )
                .await
            {
                Ok(pages) => {
                    succeeded += 1;
                    info!(
                        connection_id = cal.notion_connection_id,
                        workspace = %cal.workspace_name,
                        calendar = %cal.display_name,
                        database_id = %cal.database_id,
                        events = pages.len(),
                        "pull succeeded"
                    );
                    self.log_diff(cal.id, &pages).await;
                    self.clear_pull_failures(cal.id).await;
                    self.note_source_available(&cal).await;
                }
                Err(RefreshError::Unauthorized) => {
                    failed += 1;
                    error!(
                        connection_id = cal.notion_connection_id,
                        workspace = %cal.workspace_name,
                        calendar = %cal.display_name,
                        database_id = %cal.database_id,
                        "pull failed: unauthorized (token revoked?)"
                    );
                    self.mark_notion_connection_invalid_and_notify(cal.notion_connection_id)
                        .await;
                }
                Err(e) => {
                    failed += 1;
                    if let RefreshError::SourceUnavailable(state) = e {
                        self.note_source_unavailable(&cal, state).await;
                    }
                    self.log_pull_failure(&cal, &e).await;
                }
            }
        }
        info!(
            succeeded,
            failed, skipped, "refresh_all: pull cycle completed"
        );
    }

    /// Refresh the cache for a single Notion page after we have written it.
    ///
    /// The write paths used to call `refresh_by_data_source`, which re-reads
    /// the entire database: for a 2733-row calendar that is 28 paginated
    /// requests and about fourteen seconds, all of it before the user's own
    /// create or edit returns. Only one row actually changed, so fetch that
    /// row and upsert it.
    ///
    /// Returns false when the page could not be refreshed on its own, so the
    /// caller can fall back to a full refresh rather than leave a stale cache.
    pub async fn refresh_single_page(&self, cal: &CalendarRow, notion_page_id: &str) -> bool {
        let url = format!("{}/v1/pages/{}", self.notion_api_base_url, notion_page_id);
        let resp = match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", cal.notion_access_token))
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                // Notion usually reports a deleted page as `in_trash`, but a
                // hard delete 404s. Falling back to a full re-query would
                // reach the same conclusion after fetching every row, so draw
                // it here instead.
                info!(calendar_id = cal.id, notion_page_id, "single page gone, dropping from cache");
                self.drop_page_from_cache(cal.id, notion_page_id).await;
                return true;
            }
            Ok(r) => {
                warn!(notion_url = %url, status = %r.status(), "<- Notion API error response (single page)");
                return false;
            }
            Err(e) => {
                warn!(notion_url = %url, error = %e, "<- Notion API request failed (single page)");
                return false;
            }
        };

        let page: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!(notion_url = %url, error = %e, "could not parse single page response");
                return false;
            }
        };

        // An archived page, or one whose date was just cleared, is no longer an
        // event. Removing it here keeps a delete as cheap as a create.
        let archived = page
            .get("archived")
            .or_else(|| page.get("in_trash"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let parsed = page_to_event(&page, &cal.date_property, &cal.column_mapping);

        match (archived, parsed) {
            (false, Some(event)) => {
                self.upsert_page_state(cal.id, &event).await;
                info!(calendar_id = cal.id, notion_page_id, "single page upserted");
            }
            _ => {
                if !self.drop_page_from_cache(cal.id, notion_page_id).await {
                    return false;
                }
                info!(calendar_id = cal.id, notion_page_id, archived, "single page removed from cache");
            }
        }
        true
    }

    async fn drop_page_from_cache(&self, calendar_id: i64, notion_page_id: &str) -> bool {
        match sqlx::query(
            "DELETE FROM calendar_page_state WHERE calendar_id = $1 AND notion_page_id = $2",
        )
        .bind(calendar_id)
        .bind(notion_page_id)
        .execute(&self.db)
        .await
        {
            Ok(_) => true,
            Err(e) => {
                error!(calendar_id, error = %e, "failed to drop page from cache");
                false
            }
        }
    }

    /// Handle a webhook that names the page it is about.
    ///
    /// Notion sends `entity.id` on every page event — verified against 112
    /// production webhooks, where it was present in all of them — so a change
    /// to one row costs one request instead of re-reading the whole data
    /// source page by page. Falls back to the full re-query when the single
    /// read fails, so a surprise never silently loses the update.
    pub async fn refresh_page_by_data_source(&self, data_source_id: &str, page_id: &str) {
        let Some(cal) = self.calendar_by_data_source_id(data_source_id).await else {
            info!(
                data_source_id,
                "webhook event for untracked data source, ignoring"
            );
            return;
        };
        if self.refresh_single_page(&cal, page_id).await {
            return;
        }
        warn!(
            calendar_id = cal.id,
            page_id, "single-page webhook refresh failed, falling back to full re-query"
        );
        self.refresh_by_data_source(data_source_id).await;
    }

    pub async fn refresh_by_data_source(&self, data_source_id: &str) {
        let Some(cal) = self.calendar_by_data_source_id(data_source_id).await else {
            info!(
                data_source_id,
                "webhook event for untracked data source, ignoring"
            );
            return;
        };
        match self
            .refresh_db(
                &cal.data_source_id,
                &cal.date_property,
                &cal.notion_access_token,
                &cal.column_mapping,
            )
            .await
        {
            Ok(pages) => {
                info!(
                    connection_id = cal.notion_connection_id,
                    workspace = %cal.workspace_name,
                    calendar = %cal.display_name,
                    database_id = %cal.database_id,
                    events = pages.len(),
                    "pull succeeded (webhook-triggered)"
                );
                self.log_diff(cal.id, &pages).await;
                self.clear_pull_failures(cal.id).await;
            }
            Err(RefreshError::Unauthorized) => {
                error!(
                    connection_id = cal.notion_connection_id,
                    workspace = %cal.workspace_name,
                    calendar = %cal.display_name,
                    database_id = %cal.database_id,
                    "pull failed (webhook-triggered): unauthorized (token revoked?)"
                );
                self.mark_notion_connection_invalid_and_notify(cal.notion_connection_id)
                    .await;
            }
            Err(e) => self.log_pull_failure(&cal, &e).await,
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

    /// Live property schema of a data source, keyed by lowercased name:
    /// `lowercased -> (real name, notion type)`.
    pub async fn get_data_source_properties(
        &self,
        data_source_id: &str,
        notion_token: &str,
    ) -> HashMap<String, (String, String)> {
        let url = format!(
            "{}/v1/data_sources/{}",
            self.notion_api_base_url, data_source_id
        );
        let resp = match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                error!(notion_url = %url, status = %r.status(), "<- Notion API error response (data source schema)");
                return HashMap::new();
            }
            Err(e) => {
                error!(notion_url = %url, error = %e, "<- Notion API request failed (data source schema)");
                return HashMap::new();
            }
        };
        let data: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return HashMap::new(),
        };
        data.get("properties")
            .and_then(|p| p.as_object())
            .map(|props| {
                props
                    .iter()
                    .filter_map(|(name, def)| {
                        let ptype = def.get("type")?.as_str()?;
                        Some((name.to_lowercase(), (name.clone(), ptype.to_string())))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Resolve the real name of the title property to write into.
///
/// Writing to a literal `"Name"` was the single biggest source of write
/// failures: Notion rejects the whole request with `"Name is not a property
/// that exists."` whenever the database's title column is called anything else,
/// which is most non-English databases. Prefer the mapped name, then whatever
/// property the schema actually reports as the title.
fn title_property_name(
    schema: &HashMap<String, (String, String)>,
    mapping: &ColumnMapping,
) -> Option<String> {
    if let Some(mapped) = mapping.name_for(Field::Title) {
        if let Some((real_name, _)) = schema.get(&mapped.to_lowercase()) {
            return Some(real_name.clone());
        }
        warn!(
            requested = mapped,
            "mapped title property not found in schema, falling back to the title-typed property"
        );
    }
    schema
        .values()
        .find(|(_, ptype)| ptype == "title")
        .map(|(real_name, _)| real_name.clone())
}

fn optional_event_properties(
    schema: &HashMap<String, (String, String)>,
    fields: &ExtraEventFields,
    mapping: &ColumnMapping,
) -> serde_json::Map<String, serde_json::Value> {
    let mut properties = serde_json::Map::new();

    // Look a field up by its mapped name, falling back to the legacy English
    // key so an unconfigured calendar writes exactly where it used to.
    let slot = |field: Field, legacy: &str| -> Option<(String, String)> {
        let key = mapping
            .name_for(field)
            .map(|n| n.to_lowercase())
            .unwrap_or_else(|| legacy.to_string());
        schema.get(&key).cloned()
    };

    if let Some(location) = fields.location {
        if let Some((name, ptype)) = slot(Field::Location, "location").as_ref() {
            let value = match ptype.as_str() {
                "rich_text" => {
                    Some(serde_json::json!({ "rich_text": [{ "text": { "content": location } }] }))
                }
                "url" => Some(serde_json::json!({ "url": location })),
                _ => None,
            };
            if let Some(v) = value {
                properties.insert(name.clone(), v);
            }
        }
    }
    if let Some(notes) = fields.notes {
        if let Some((name, ptype)) = slot(Field::Notes, "notes").as_ref() {
            if ptype == "rich_text" {
                properties.insert(
                    name.clone(),
                    serde_json::json!({ "rich_text": [{ "text": { "content": notes } }] }),
                );
            }
        }
    }
    if let Some(priority) = fields.priority {
        if let Some((name, ptype)) = slot(Field::Priority, "priority").as_ref() {
            if ptype == "select" {
                if let Some(option_name) = priority_number_to_select_name(priority) {
                    properties.insert(
                        name.clone(),
                        serde_json::json!({ "select": { "name": option_name } }),
                    );
                }
            }
        }
    }
    if let Some(busy) = fields.busy {
        // Busy accepts either shape, so take the first candidate name the
        // schema actually has and write whichever form it is.
        let found = mapping
            .names_for(Field::Busy)
            .into_iter()
            .find_map(|name| schema.get(&name.to_lowercase()));
        if let Some((name, ptype)) = found {
            match ptype.as_str() {
                "checkbox" => {
                    properties.insert(name.clone(), serde_json::json!({ "checkbox": busy }));
                }
                "select" => {
                    properties.insert(
                        name.clone(),
                        serde_json::json!({ "select": { "name": if busy { "Busy" } else { "Free" } } }),
                    );
                }
                _ => {}
            }
        }
    }
    if let Some(minutes) = fields.reminder_minutes {
        if let Some((name, ptype)) = slot(Field::Reminder, "reminder").as_ref() {
            if ptype == "number" {
                properties.insert(name.clone(), serde_json::json!({ "number": minutes }));
            }
        }
    }
    if let Some(minutes) = fields.travel_minutes {
        if let Some((name, ptype)) = slot(Field::TravelMinutes, "travel time").as_ref() {
            if ptype == "number" {
                properties.insert(name.clone(), serde_json::json!({ "number": minutes }));
            }
        }
    }

    properties
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub async fn notion_create_event(
        &self,
        data_source_id: &str,
        date_property: &str,
        notion_token: &str,
        title: &str,
        start: &str,
        end: Option<&str>,
        extra: &ExtraEventFields<'_>,
        mapping: &ColumnMapping,
    ) -> Result<String, String> {
        let schema = self
            .get_data_source_properties(data_source_id, notion_token)
            .await;

        let mut properties = serde_json::Map::new();
        let title_name = title_property_name(&schema, mapping)
            .ok_or_else(|| "Notion database has no title property".to_string())?;
        info!(
            data_source_id,
            title_property = %title_name,
            date_property,
            "resolved properties for create"
        );
        properties.insert(
            title_name,
            serde_json::json!({ "title": [{ "text": { "content": title } }] }),
        );
        properties.insert(
            date_property.to_string(),
            serde_json::json!({ "date": self.date_property_value(start, end) }),
        );
        properties.extend(optional_event_properties(&schema, extra, mapping));

        let body = serde_json::json!({
            "parent": { "type": "data_source_id", "data_source_id": data_source_id },
            "properties": properties,
        });

        let create_url = format!("{}/v1/pages", self.notion_api_base_url);
        info!(notion_method = "POST", notion_url = %create_url, title = %title, "-> Notion API request (create event)");
        let resp = self
            .client
            .post(&create_url)
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

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Parse failed: {}", e))?;
        let page_id = data
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        info!(status = %resp_status, page_id = ?page_id, "<- Notion API response (create event)");
        page_id.ok_or_else(|| "Notion response missing page id".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn notion_update_event(
        &self,
        page_id: &str,
        data_source_id: &str,
        date_property: &str,
        notion_token: &str,
        title: Option<&str>,
        start: Option<&str>,
        end: Option<Option<&str>>,
        extra: &ExtraEventFields<'_>,
        mapping: &ColumnMapping,
    ) -> Result<(), String> {
        let schema = self
            .get_data_source_properties(data_source_id, notion_token)
            .await;

        let mut properties = serde_json::Map::new();
        if let Some(t) = title {
            let title_name = title_property_name(&schema, mapping)
                .ok_or_else(|| "Notion database has no title property".to_string())?;
            info!(
                data_source_id,
                title_property = %title_name,
                "resolved properties for update"
            );
            properties.insert(
                title_name,
                serde_json::json!({ "title": [{ "text": { "content": t } }] }),
            );
        }
        if let Some(s) = start {
            properties.insert(
                date_property.to_string(),
                serde_json::json!({ "date": self.date_property_value(s, end.flatten()) }),
            );
        }
        properties.extend(optional_event_properties(&schema, extra, mapping));

        if properties.is_empty() {
            return Ok(());
        }

        let body = serde_json::json!({ "properties": properties });
        self.patch_page(page_id, notion_token, &body).await
    }

    pub async fn notion_delete_event(
        &self,
        page_id: &str,
        notion_token: &str,
    ) -> Result<(), String> {
        self.patch_page(
            page_id,
            notion_token,
            &serde_json::json!({ "in_trash": true }),
        )
        .await
    }

    async fn patch_page(
        &self,
        page_id: &str,
        notion_token: &str,
        body: &serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{}/v1/pages/{}", self.notion_api_base_url, page_id);
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
        let url = format!("{}/v1/databases/{}", self.notion_api_base_url, db_id);
        info!(notion_method = "GET", notion_url = %url, "-> Notion API request (calendar name)");
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", notion_token))
            .header("Notion-Version", "2025-09-03")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                let status = r.status();
                let name = r
                    .json::<serde_json::Value>()
                    .await
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
    if !value.contains('T') {
        return format!(";VALUE=DATE:{}", value.replace('-', ""));
    }
    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(dt) => dt
            .with_timezone(&chrono::Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string(),
        Err(_) => {
            let stripped = value.replace(['-', ':'], "");
            let mut parts = stripped.splitn(2, 'T');
            let date = parts.next().unwrap_or("");
            let mut time = parts.next().unwrap_or("");
            if let Some(dot) = time.find('.') {
                time = &time[..dot];
            }
            format!("{date}T{time}Z")
        }
    }
}

/// `DTEND` for an all-day event is *exclusive* under RFC 5545, while Notion's
/// range end is *inclusive*. Emitting Notion's date unchanged therefore drops
/// the final day, and a single-day range (start == end) becomes a zero-length
/// event that some clients refuse to draw at all. Advance the date by one day
/// so the rendered span matches what the user set in Notion.
fn all_day_dtend(notion_end: &str) -> Option<String> {
    let date = chrono::NaiveDate::parse_from_str(notion_end, "%Y-%m-%d").ok()?;
    let exclusive = date.succ_opt()?;
    Some(format!(";VALUE=DATE:{}", exclusive.format("%Y%m%d")))
}

/// Render events as an iCalendar document.
///
/// Deliberately takes no calendar identifier: event UIDs come from the
/// Notion page id alone, so the same event has the same UID no matter which
/// route served it.
pub fn build_ics(name: &str, pages: &[PageInfo]) -> String {
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
        let all_day = !page.start.contains('T');
        let end = match page.end.as_deref() {
            // All-day ranges need the inclusive -> exclusive shift; timed
            // events carry an exact instant and are emitted as-is.
            Some(e) if all_day => all_day_dtend(e).unwrap_or_default(),
            Some(e) => ics_dt(e),
            None => String::new(),
        };

        if start.is_empty() {
            continue;
        }

        ics.push_str("BEGIN:VEVENT\r\n");
        // The Notion page id alone, because it is the only identifier here
        // that is both globally unique and stable. The first argument to this
        // function used to be part of the UID, and callers pass four different
        // things for it — the Notion database id, the calendar's public_id, or
        // the literal "all" — so one event could come back under three
        // identities depending on which route served it. Worse, public_id is
        // regenerated whenever a calendar is deleted and recreated, which
        // silently renames every event: clients key on UID, so they keep the
        // old copies and add the new ones. That is what a customer reported as
        // events appearing four and then five times.
        //
        // Changing the scheme renames every event once, so expect a single
        // round of duplicates on the deploy that ships this, and never again.
        ics.push_str(&format!(
            "UID:{}@notion-caldav.opendiy.vn\r\n",
            page.id.replace("-", "")
        ));
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
        ics.push_str(&format!("URL:{}\r\n", escape_ics(&page.url)));
        if let Some(notes) = &page.notes {
            ics.push_str(&format!("DESCRIPTION:{}\r\n", escape_ics(notes)));
        }
        if let Some(location) = &page.location {
            ics.push_str(&format!("LOCATION:{}\r\n", escape_ics(location)));
        }
        if let Some(priority) = page.priority {
            ics.push_str(&format!("PRIORITY:{}\r\n", priority));
        }
        if let Some(busy) = page.busy {
            ics.push_str(&format!(
                "TRANSP:{}\r\n",
                if busy { "OPAQUE" } else { "TRANSPARENT" }
            ));
        }
        if let Some(rrule) = &page.repeat_rule {
            ics.push_str(&format!("RRULE:{}\r\n", rrule));
        }
        for email in &page.attendees {
            ics.push_str(&format!(
                "ATTENDEE;CN={}:mailto:{}\r\n",
                escape_ics(email),
                email
            ));
        }
        if let Some(minutes) = page.travel_minutes {
            ics.push_str(&format!(
                "X-APPLE-TRAVEL-DURATION;VALUE=DURATION:PT{}M\r\n",
                minutes
            ));
        }
        if let Some(minutes) = page.reminder_minutes {
            ics.push_str("BEGIN:VALARM\r\n");
            ics.push_str("ACTION:DISPLAY\r\n");
            ics.push_str(&format!("DESCRIPTION:{}\r\n", escape_ics(&page.title)));
            ics.push_str(&format!("TRIGGER:-PT{}M\r\n", minutes));
            ics.push_str("END:VALARM\r\n");
        }
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
    let mut notes = None;
    let mut location = None;
    let mut priority = None;
    let mut busy = None;
    let mut reminder_minutes = None;
    let mut uid = default_id.to_string();
    let mut in_valarm = false;

    fn unescape(s: &str) -> String {
        s.replace("\\,", ",")
            .replace("\\;", ";")
            .replace("\\n", "\n")
            .replace("\\\\", "\\")
    }

    for line in ics_content.lines() {
        let line = line.trim();
        if line == "BEGIN:VALARM" {
            in_valarm = true;
        } else if line == "END:VALARM" {
            in_valarm = false;
        } else if in_valarm {
            if let Some(rest) = line
                .strip_prefix("TRIGGER:-PT")
                .and_then(|s| s.strip_suffix('M'))
            {
                reminder_minutes = rest.parse::<i64>().ok();
            }
        } else if let Some(rest) = line.strip_prefix("SUMMARY:") {
            title = unescape(rest);
        } else if let Some(rest) = line.strip_prefix("DTSTART:") {
            start = parse_ics_date(rest);
        } else if let Some(rest) = line.strip_prefix("DTSTART;VALUE=DATE:") {
            start = parse_ics_date(rest);
        } else if let Some(rest) = line.strip_prefix("DTEND:") {
            end = Some(parse_ics_date(rest));
        } else if let Some(rest) = line.strip_prefix("DTEND;VALUE=DATE:") {
            end = Some(parse_ics_date(rest));
        } else if let Some(rest) = line.strip_prefix("DESCRIPTION:") {
            let text = unescape(rest);
            if !text.is_empty() {
                notes = Some(text);
            }
        } else if let Some(rest) = line.strip_prefix("LOCATION:") {
            let text = unescape(rest);
            if !text.is_empty() {
                location = Some(text);
            }
        } else if let Some(rest) = line.strip_prefix("PRIORITY:") {
            priority = rest.parse::<u8>().ok();
        } else if let Some(rest) = line.strip_prefix("TRANSP:") {
            busy = match rest {
                "OPAQUE" => Some(true),
                "TRANSPARENT" => Some(false),
                _ => None,
            };
        } else if let Some(rest) = line.strip_prefix("UID:") {
            uid = rest.to_string();
        }
    }

    PageInfo {
        id: uid,
        title,
        start,
        end,
        url: String::new(),
        last_edited: chrono::Utc::now().to_rfc3339(),
        location,
        notes,
        priority,
        busy,
        reminder_minutes,
        travel_minutes: None,
        repeat_rule: None,
        attendees: Vec::new(),
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
        <!-- Clients ask whether they may write before offering a calendar
             as a destination for new events; omitting this left Apple
             Calendar with nothing to go on. -->
        <D:current-user-privilege-set>
          <D:privilege><D:read/></D:privilege>
          <D:privilege><D:write/></D:privilege>
          <D:privilege><D:write-content/></D:privilege>
          <D:privilege><D:write-properties/></D:privilege>
          <D:privilege><D:bind/></D:privilege>
          <D:privilege><D:unbind/></D:privilege>
        </D:current-user-privilege-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
        prefix = prefix,
        display_name = display_name
    )
}

pub fn build_propfind_calendar_with_events(
    prefix: &str,
    display_name: &str,
    pages: &[PageInfo],
) -> String {
    let mut xml = String::new();
    xml.push_str(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>"#,
    );
    xml.push_str(prefix);
    xml.push_str(
        r#"</D:href>
    <D:propstat>
      <D:prop>
        <D:displayname>"#,
    );
    xml.push_str(display_name);
    xml.push_str(
        r#"</D:displayname>
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
  </D:response>"#,
    );

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

pub fn build_report_response(
    prefix: &str,
    calendar_name: &str,
    pages: &[PageInfo],
) -> String {
    let mut xml = String::new();
    xml.push_str(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#,
    );

    for page in pages {
        let clean_id = page.id.replace("-", "");
        let etag = &page.last_edited;
        let ics_body = build_ics(calendar_name, std::slice::from_ref(page));
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

pub async fn get_public_id_for_host(
    headers: &axum::http::HeaderMap,
    state: &AppState,
) -> Option<String> {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("").trim();
    let db_id = match host_name {
        "calendar.opendiy.vn" => Some("4cb38c7656ae483d8ee5650d9fb02108"),
        "mytime.opendiy.vn" => Some("39e6a94a90a680da85d2c29e3c52ed8e"),
        _ => None,
    }?;
    state
        .calendar_by_db_id(db_id)
        .await
        .map(|cal| cal.public_id)
}

pub async fn handle_calendar_impl(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    state: AppState,
    public_id: String,
    prefix: String,
    _body: String,
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
        state
            .get_calendar_name(&cal.database_id, &cal.notion_access_token)
            .await
    } else {
        cal.display_name.clone()
    };
    if (method == axum::http::Method::PUT
        || method == axum::http::Method::DELETE
        || method.as_str() == "PROPPATCH")
        && state.caldav_allow_writes != CaldavAllowWrites::True
    {
        return axum::http::StatusCode::FORBIDDEN.into_response();
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
        let pages = state.pages_for_calendar(cal.id).await;
        let body = build_ics(&name, &pages);
        return (
            [(header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
            body,
        )
            .into_response();
    }

    if method.as_str() == "PROPFIND" {
        let depth = headers
            .get("depth")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("0");
        let body = if depth == "1" {
            let pages = state.pages_for_calendar(cal.id).await;
            build_propfind_calendar_with_events(&prefix, &name, &pages)
        } else {
            build_propfind_calendar(&prefix, &name)
        };
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        )
            .into_response();
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
        )
            .into_response();
    }

    if method.as_str() == "REPORT" {
        let pages = state.pages_for_calendar(cal.id).await;
        let body = build_report_response(&prefix, &name, &pages);
        return (
            axum::http::StatusCode::MULTI_STATUS,
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        )
            .into_response();
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
        state
            .get_calendar_name(&cal.database_id, &cal.notion_access_token)
            .await
    } else {
        cal.display_name.clone()
    };
    let event_id_clean = event_id
        .strip_suffix(".ics")
        .unwrap_or(&event_id)
        .to_string();
    if (method == axum::http::Method::PUT
        || method == axum::http::Method::DELETE
        || method.as_str() == "PROPPATCH")
        && state.caldav_allow_writes != CaldavAllowWrites::True
    {
        return axum::http::StatusCode::FORBIDDEN.into_response();
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
        let pages = state.pages_for_calendar(cal.id).await;
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_ics(&name, std::slice::from_ref(page));
            info!(status = 200, found = true, "CalDAV event GET");
            return (
                [(header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
                body,
            )
                .into_response();
        } else {
            info!(status = 404, found = false, "CalDAV event GET not found");
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    if method.as_str() == "PROPFIND" {
        let pages = state.pages_for_calendar(cal.id).await;
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_propfind_event(&prefix, &event_id_clean, page);
            return (
                axum::http::StatusCode::MULTI_STATUS,
                [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
                body,
            )
                .into_response();
        } else {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    if method == axum::http::Method::PUT {
        let new_page = parse_ics_to_page_info(&body, &event_id_clean);
        let existing_id = state
            .pages_for_calendar(cal.id)
            .await
            .iter()
            .find(|p| matches_id(&p.id, &event_id_clean))
            .map(|p| p.id.clone());
        let existing_id = match existing_id {
            Some(id) => Some(id),
            None => state.lookup_caldav_uid(cal.id, &event_id_clean).await,
        };
        let is_update = existing_id.is_some();
        let action = if is_update { "update" } else { "create" };
        if crate::billing::enforce_quota(&state, cal.user_id)
            .await
            .is_err()
        {
            state
                .log_sync(
                    cal.id,
                    "caldav",
                    action,
                    &event_id_clean,
                    "",
                    "error",
                    "daily quota exceeded",
                )
                .await;
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
        let extra = ExtraEventFields {
            location: new_page.location.as_deref(),
            notes: new_page.notes.as_deref(),
            priority: new_page.priority,
            busy: new_page.busy,
            reminder_minutes: new_page.reminder_minutes,
            travel_minutes: None,
        };
        let result = if let Some(page_id) = existing_id {
            state
                .notion_update_event(
                    &page_id,
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    Some(&new_page.title),
                    Some(&new_page.start),
                    Some(new_page.end.as_deref()),
                    &extra,
                    &cal.column_mapping,
                )
                .await
                .map(|_| (axum::http::StatusCode::NO_CONTENT, page_id))
        } else {
            match state
                .notion_create_event(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                    &new_page.title,
                    &new_page.start,
                    new_page.end.as_deref(),
                    &extra,
                    &cal.column_mapping,
                )
                .await
            {
                Ok(page_id) => {
                    state
                        .store_caldav_uid_mapping(cal.id, &event_id_clean, &page_id)
                        .await;
                    Ok((axum::http::StatusCode::CREATED, page_id))
                }
                Err(e) => Err(e),
            }
        };
        return match result {
            Ok((status, page_id)) => {
                state
                    .log_sync(
                        cal.id,
                        "caldav",
                        action,
                        &event_id_clean,
                        &page_id,
                        "ok",
                        "",
                    )
                    .await;
                // Refresh just the row we wrote; fall back to the whole data
                // source only if that single read fails.
                if !state.refresh_single_page(&cal, &page_id).await {
                    state.refresh_by_data_source(&cal.data_source_id).await;
                }
                status.into_response()
            }
            Err(e) => {
                error!(
                    "CalDAV PUT event {} failed to sync to Notion: {}",
                    event_id_clean, e
                );
                state
                    .log_sync(cal.id, "caldav", action, &event_id_clean, "", "error", &e)
                    .await;
                axum::http::StatusCode::BAD_GATEWAY.into_response()
            }
        };
    }

    if method == axum::http::Method::DELETE {
        let existing_id = state
            .pages_for_calendar(cal.id)
            .await
            .iter()
            .find(|p| matches_id(&p.id, &event_id_clean))
            .map(|p| p.id.clone());
        let existing_id = match existing_id {
            Some(id) => Some(id),
            None => state.lookup_caldav_uid(cal.id, &event_id_clean).await,
        };
        let Some(page_id) = existing_id else {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        };
        return match state
            .notion_delete_event(&page_id, &cal.notion_access_token)
            .await
        {
            Ok(()) => {
                state
                    .log_sync(
                        cal.id,
                        "caldav",
                        "delete",
                        &event_id_clean,
                        &page_id,
                        "ok",
                        "",
                    )
                    .await;
                state
                    .delete_caldav_uid_mapping(cal.id, &event_id_clean)
                    .await;
                // Refresh just the row we wrote; fall back to the whole data
                // source only if that single read fails.
                if !state.refresh_single_page(&cal, &page_id).await {
                    state.refresh_by_data_source(&cal.data_source_id).await;
                }
                axum::http::StatusCode::NO_CONTENT.into_response()
            }
            Err(e) => {
                error!(
                    "CalDAV DELETE event {} failed to sync to Notion: {}",
                    event_id_clean, e
                );
                state
                    .log_sync(
                        cal.id,
                        "caldav",
                        "delete",
                        &event_id_clean,
                        &page_id,
                        "error",
                        &e,
                    )
                    .await;
                axum::http::StatusCode::BAD_GATEWAY.into_response()
            }
        };
    }

    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

fn extract_basic_auth(headers: &axum::http::HeaderMap) -> Option<(String, String)> {
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok())?;
    let basic_val = auth_header.strip_prefix("Basic ")?;
    let decoded = base64_light::base64_decode_str(basic_val);
    let mut parts = decoded.splitn(2, ':');
    let username = parts.next()?.to_string();
    let password = parts.next()?.to_string();
    Some((username, password))
}

fn extract_path_public_id(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/cal/")?;
    let seg = rest.split('/').next().unwrap_or("");
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_string())
    }
}

pub async fn handle_path_calendar(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Path(public_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", public_id);
    let res = handle_calendar_impl(method, headers, state, public_id, prefix, body)
        .await
        .into_response();
    add_caldav_headers(res)
}

pub async fn handle_path_calendar_event(
    method: axum::http::Method,
    State(state): State<AppState>,
    Path((public_id, event_id)): Path<(String, String)>,
    body: String,
) -> impl IntoResponse {
    let prefix = format!("/cal/{}/", public_id);
    let res = handle_calendar_event_impl(method, state, public_id, event_id, prefix, body)
        .await
        .into_response();
    add_caldav_headers(res)
}

pub async fn handle_host_calendar(
    method: axum::http::Method,
    uri: axum::http::Uri,
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
        let res = handle_calendar_impl(method, headers, state, public_id, prefix, body)
            .await
            .into_response();
        add_caldav_headers(res)
    } else {
        if method == axum::http::Method::GET || method == axum::http::Method::HEAD {
            let mut synthetic_request = axum::http::Request::builder()
                .method(method.clone())
                .uri(uri.clone())
                .body(axum::body::Body::empty())
                .expect("method/uri from a real incoming request are always valid");
            *synthetic_request.headers_mut() = headers.clone();
            let leptos_options = state.leptos_options.clone();
            let handler =
                leptos_axum::render_app_to_stream(move || app::shell(leptos_options.clone()));
            return handler(synthetic_request).await.into_response();
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
        <!-- Clients ask whether they may write before offering a calendar
             as a destination for new events; omitting this left Apple
             Calendar with nothing to go on. -->
        <D:current-user-privilege-set>
          <D:privilege><D:read/></D:privilege>
          <D:privilege><D:write/></D:privilege>
          <D:privilege><D:write-content/></D:privilege>
          <D:privilege><D:write-properties/></D:privilege>
          <D:privilege><D:bind/></D:privilege>
          <D:privilege><D:unbind/></D:privilege>
        </D:current-user-privilege-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;
            return (
                axum::http::StatusCode::MULTI_STATUS,
                [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
                body,
            )
                .into_response();
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
        let res = handle_calendar_event_impl(method, state, public_id, event_id, prefix, body)
            .await
            .into_response();
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
    headers.insert(
        "DAV",
        axum::http::HeaderValue::from_static("1, 3, calendar-access"),
    );
    headers.insert(
        "Allow",
        axum::http::HeaderValue::from_static(
            "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
        ),
    );
    response
}

async fn handle_well_known(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let query = headers
        .get("x-request-query")
        .map(|_| "has-query")
        .unwrap_or("")
        .to_string();
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
            (
                header::LOCATION,
                axum::http::HeaderValue::from_static("/principals/"),
            ),
            (
                axum::http::HeaderName::from_static("dav"),
                axum::http::HeaderValue::from_static("1, 3, calendar-access"),
            ),
            (
                axum::http::HeaderName::from_static("allow"),
                axum::http::HeaderValue::from_static(
                    "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                ),
            ),
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
                (
                    axum::http::HeaderName::from_static("dav"),
                    axum::http::HeaderValue::from_static("1, 3, calendar-access"),
                ),
                (
                    axum::http::HeaderName::from_static("allow"),
                    axum::http::HeaderValue::from_static(
                        "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                    ),
                ),
            ],
        )
            .into_response();
    }
    if method.as_str() == "PROPFIND" {
        let username = auth
            .map(|a| a.0.username.clone())
            .unwrap_or_else(|| "user".to_string());
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
        <!-- Clients ask whether they may write before offering a calendar
             as a destination for new events; omitting this left Apple
             Calendar with nothing to go on. -->
        <D:current-user-privilege-set>
          <D:privilege><D:read/></D:privilege>
          <D:privilege><D:write/></D:privilege>
          <D:privilege><D:write-content/></D:privilege>
          <D:privilege><D:write-properties/></D:privilege>
          <D:privilege><D:bind/></D:privilege>
          <D:privilege><D:unbind/></D:privilege>
        </D:current-user-privilege-set>
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
                (
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/xml; charset=utf-8"),
                ),
                (
                    axum::http::HeaderName::from_static("dav"),
                    axum::http::HeaderValue::from_static("1, 3, calendar-access"),
                ),
                (
                    axum::http::HeaderName::from_static("allow"),
                    axum::http::HeaderValue::from_static(
                        "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                    ),
                ),
            ],
            body,
        )
            .into_response();
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
                (
                    axum::http::HeaderName::from_static("dav"),
                    axum::http::HeaderValue::from_static("1, 3, calendar-access"),
                ),
                (
                    axum::http::HeaderName::from_static("allow"),
                    axum::http::HeaderValue::from_static(
                        "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                    ),
                ),
            ],
        )
            .into_response();
    }
    let owner_calendars: Vec<CalendarRow> = match &auth {
        Some(a) => {
            let cals = state.calendars_for_user(a.0.user_id).await;
            match &a.0.scoped_public_id {
                Some(scoped) => cals
                    .into_iter()
                    .filter(|c| &c.public_id == scoped)
                    .collect(),
                None => cals,
            }
        }
        None => Vec::new(),
    };

    if method.as_str() == "PROPFIND" {
        let host_public_id = get_public_id_for_host(&headers, &state).await;
        let all_cals = owner_calendars;
        let cals_to_return: Vec<CalendarRow> = if let Some(public_id) = &host_public_id {
            all_cals
                .into_iter()
                .filter(|c| &c.public_id == public_id)
                .collect()
        } else {
            all_cals
        };

        let mut responses_xml = String::new();
        for cal in cals_to_return {
            let name = if cal.display_name.is_empty() {
                state
                    .get_calendar_name(&cal.database_id, &cal.notion_access_token)
                    .await
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
                (
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/xml; charset=utf-8"),
                ),
                (
                    axum::http::HeaderName::from_static("dav"),
                    axum::http::HeaderValue::from_static("1, 3, calendar-access"),
                ),
                (
                    axum::http::HeaderName::from_static("allow"),
                    axum::http::HeaderValue::from_static(
                        "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                    ),
                ),
            ],
            body,
        )
            .into_response();
    }
    if method.as_str() == "REPORT" {
        let host_public_id = get_public_id_for_host(&headers, &state).await;
        let all_cals = owner_calendars;
        let cals_to_return: Vec<CalendarRow> = if let Some(public_id) = &host_public_id {
            all_cals
                .into_iter()
                .filter(|c| &c.public_id == public_id)
                .collect()
        } else {
            all_cals
        };

        let mut xml = String::new();
        xml.push_str(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">"#,
        );

        for cal in cals_to_return {
            let name = if cal.display_name.is_empty() {
                state
                    .get_calendar_name(&cal.database_id, &cal.notion_access_token)
                    .await
            } else {
                cal.display_name.clone()
            };
            let prefix = if host_public_id.is_some() {
                "/".to_string()
            } else {
                format!("/cal/{}/", cal.public_id)
            };
            let pages = state.pages_for_calendar(cal.id).await;
            for page in pages {
                let clean_id = page.id.replace("-", "");
                let etag = &page.last_edited;
                let ics_body = build_ics(&name, std::slice::from_ref(&page));
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
                (
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/xml; charset=utf-8"),
                ),
                (
                    axum::http::HeaderName::from_static("dav"),
                    axum::http::HeaderValue::from_static("1, 3, calendar-access"),
                ),
                (
                    axum::http::HeaderName::from_static("allow"),
                    axum::http::HeaderValue::from_static(
                        "GET, HEAD, PROPFIND, REPORT, PUT, DELETE, OPTIONS, PROPPATCH",
                    ),
                ),
            ],
            xml,
        )
            .into_response();
    }
    axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response()
}

async fn auth_middleware(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
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

    let is_landing_page_request = (method == axum::http::Method::GET
        || method == axum::http::Method::HEAD)
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

    let Some((user_id, scoped_public_id)) =
        state.verify_caldav_credentials(&username, &password).await
    else {
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

    let target_public_id = match extract_path_public_id(&path) {
        Some(id) => Some(id),
        None => get_public_id_for_host(&headers, &state).await,
    };
    if let Some(target) = &target_public_id {
        let authorized = match &scoped_public_id {
            Some(scoped) => target == scoped,
            None => {
                matches!(state.calendar_by_public_id(target).await, Some(cal) if cal.user_id == user_id)
            }
        };
        if !authorized {
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
    request.extensions_mut().insert(AuthenticatedCaldavUser {
        user_id,
        username,
        scoped_public_id,
    });

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

fn routes_dispatched_outside_leptos_routes() -> Vec<String> {
    [
        "/",
        "/connect/notion",
        "/connect/notion/databases",
        "/me",
        "/app/{public_id}",
        "/me/calendars/{public_id}/log",
        "/me/calendars/{public_id}/mapping",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub fn create_app(
    state: AppState,
    oidc_client: axum_oidc::OidcClient<axum_oidc::EmptyAdditionalClaims>,
    app_config: crate::session::AppConfig,
) -> Router {
    use crate::session::SessionWrapper;
    use axum::error_handling::HandleErrorLayer;
    use axum_oidc::{
        error::MiddlewareError, handle_oidc_redirect, EmptyAdditionalClaims, OidcAuthLayer,
        OidcLoginLayer,
    };
    use leptos_axum::{generate_route_list_with_exclusions, LeptosRoutes};
    use tower::ServiceBuilder;

    let leptos_routes = generate_route_list_with_exclusions(
        app::App,
        Some(routes_dispatched_outside_leptos_routes()),
    );
    let leptos_options = state.leptos_options.clone();
    let leptos_options_for_login_required = leptos_options.clone();
    let db_pool_for_context = state.db.clone();
    let http_client_for_context = state.client.clone();
    let notion_api_base_url_for_context =
        app::page_shell::NotionApiBaseUrl(state.notion_api_base_url.clone());
    let app_base_url_for_context = app::page_shell::AppBaseUrl(app_config.base_url.clone());
    let paddle_configured_for_context = app::page_shell::PaddleConfigured(state.paddle.is_some());
    let mapbox_token_for_context = app::page_shell::MapboxToken(state.mapbox_token.clone());

    let oidc_login_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            e.into_response()
        }))
        .layer(OidcLoginLayer::<EmptyAdditionalClaims, SessionWrapper>::new());

    let oidc_auth_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async move {
            e.into_response()
        }))
        .layer(OidcAuthLayer::<_, SessionWrapper>::new(oidc_client));

    let caldav_routes = Router::<AppState>::new()
        .route("/cal/{public_id}", axum::routing::any(handle_path_calendar))
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
        .route("/.well-known/caldav", axum::routing::any(handle_well_known))
        .route(
            "/.well-known/caldav/",
            axum::routing::any(handle_well_known),
        )
        .route("/principals", axum::routing::any(handle_principals))
        .route("/principals/", axum::routing::any(handle_principals))
        .route(
            "/calendars/{user}",
            axum::routing::any(handle_calendars_propfind),
        )
        .route(
            "/calendars/{user}/",
            axum::routing::any(handle_calendars_propfind),
        )
        .route("/", axum::routing::any(handle_host_calendar))
        .route(
            "/{event_id}",
            axum::routing::any(handle_host_calendar_event),
        )
        .route(
            "/{event_id}/",
            axum::routing::any(handle_host_calendar_event),
        )
        .route(
            "/refresh",
            post(
                move |axum::Extension(auth): axum::Extension<AuthenticatedCaldavUser>,
                      State(state): State<AppState>| async move {
                    state.refresh_for_user(auth.user_id).await;
                    "refresh triggered"
                },
            ),
        )
        .route(
            "/cal.ics",
            get(
                move |axum::Extension(auth): axum::Extension<AuthenticatedCaldavUser>,
                      State(state): State<AppState>| async move {
                    let my_cals = state.calendars_for_user(auth.user_id).await;
                    let mut all_pages: Vec<PageInfo> = Vec::new();
                    let mut names: Vec<String> = Vec::new();
                    for cal in &my_cals {
                        all_pages.extend(state.pages_for_calendar(cal.id).await);
                        names.push(if cal.display_name.is_empty() {
                            format!("Notion {}", &cal.database_id[..8])
                        } else {
                            cal.display_name.clone()
                        });
                    }
                    let name = names.join(", ");
                    let body = build_ics(&name, &all_pages);
                    (
                        [(header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
                        body,
                    )
                        .into_response()
                },
            ),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let leptos_login_required_page = {
        let leptos_options = leptos_options_for_login_required.clone();
        let db_pool = db_pool_for_context.clone();
        let http_client = http_client_for_context.clone();
        let notion_api_base_url = notion_api_base_url_for_context.clone();
        let app_base_url = app_base_url_for_context.clone();
        let paddle_configured = paddle_configured_for_context.clone();
        let mapbox_token = mapbox_token_for_context.clone();
        move || {
            let leptos_options = leptos_options.clone();
            let db_pool = db_pool.clone();
            let http_client = http_client.clone();
            let notion_api_base_url = notion_api_base_url.clone();
            let app_base_url = app_base_url.clone();
            let paddle_configured = paddle_configured.clone();
            let mapbox_token = mapbox_token.clone();
            move |request: axum::extract::Request| {
                let leptos_options = leptos_options.clone();
                let db_pool = db_pool.clone();
                let http_client = http_client.clone();
                let notion_api_base_url = notion_api_base_url.clone();
                let app_base_url = app_base_url.clone();
                let paddle_configured = paddle_configured.clone();
                let mapbox_token = mapbox_token.clone();
                async move {
                    let handler = leptos_axum::render_app_to_stream_with_context(
                        move || {
                            leptos::prelude::provide_context(db_pool.clone());
                            leptos::prelude::provide_context(http_client.clone());
                            leptos::prelude::provide_context(notion_api_base_url.clone());
                            leptos::prelude::provide_context(app_base_url.clone());
                            leptos::prelude::provide_context(paddle_configured.clone());
                            leptos::prelude::provide_context(mapbox_token.clone());
                        },
                        move || app::shell(leptos_options.clone()),
                    );
                    handler(request).await
                }
            }
        }
    };

    let me_route = Router::new()
        .route("/me", get(leptos_login_required_page()))
        .route("/connect/notion", get(leptos_login_required_page()))
        .route(
            "/connect/notion/start",
            get(crate::pages::connect_notion::connect_notion_start),
        )
        .route(
            "/connect/notion/databases",
            get(leptos_login_required_page()).post(crate::pages::pick_databases::create_calendars),
        )
        .route(
            "/oauth/notion/callback",
            get(crate::pages::connect_notion::notion_oauth_callback),
        )
        .route(
            "/me/calendars/{public_id}/delete",
            post(crate::pages::me::delete_calendar),
        )
        .route(
            "/me/calendars/{public_id}/log",
            get(leptos_login_required_page()),
        )
        .route(
            "/me/calendars/{public_id}/mapping",
            get(leptos_login_required_page()).post(crate::pages::edit_mapping::update_mapping),
        )
        .route(
            "/app",
            get(|| async { axum::response::Redirect::to("/me") }),
        )
        .route("/app/{public_id}", get(leptos_login_required_page()))
        .route(
            "/app/{public_id}/api/events",
            get(crate::api::webview_events::handle_list_events)
                .post(crate::api::webview_events::handle_create_event),
        )
        .route(
            "/app/{public_id}/api/events/{event_id}",
            axum::routing::patch(crate::api::webview_events::handle_update_event)
                .delete(crate::api::webview_events::handle_delete_event),
        )
        .route("/billing/checkout", get(crate::billing::start_checkout))
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
        .route(
            "/webhook/notion-test",
            post(crate::webhook::handle_notion_webhook),
        )
        .route(
            "/billing/webhook",
            post(crate::billing::handle_paddle_webhook),
        )
        .route("/admin/reset-billing", post(crate::billing::reset_billing))
        .route(
            "/admin/backfill-column-mappings",
            post(crate::pages::backfill_mapping::backfill_column_mappings),
        )
        .route(
            "/admin/grant-lifetime",
            post(crate::billing::grant_lifetime_to_non_paying_users),
        )
        .route("/admin/send-test-email", post(crate::email::send_test_email))
        .route(
            "/me/account-caldav.mobileconfig",
            get(crate::pages::me::download_account_mobileconfig),
        )
        .route(
            "/me/account-caldav-qr.svg",
            get(crate::pages::me::account_caldav_qr),
        )
        .route(
            "/notion-connection/{action_token}/stop",
            get(crate::pages::connect_notion::stop_sync_by_action_token),
        )
        .nest_service(
            "/pkg",
            tower::ServiceBuilder::new()
                .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-cache"),
                ))
                .service(tower_http::services::ServeDir::new(format!(
                    "{}/{}",
                    leptos_options.site_root, leptos_options.site_pkg_dir
                ))),
        )
        .nest_service(
            "/assets",
            tower::ServiceBuilder::new()
                .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-cache"),
                ))
                .service(tower_http::services::ServeDir::new("assets")),
        )
        .nest_service(
            "/static",
            tower::ServiceBuilder::new()
                .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-cache"),
                ))
                .service(tower_http::services::ServeDir::new("static")),
        )
        .leptos_routes_with_context(
            &state,
            leptos_routes,
            move || {
                leptos::prelude::provide_context(db_pool_for_context.clone());
                leptos::prelude::provide_context(http_client_for_context.clone());
                leptos::prelude::provide_context(notion_api_base_url_for_context.clone());
                leptos::prelude::provide_context(app_base_url_for_context.clone());
                leptos::prelude::provide_context(paddle_configured_for_context.clone());
                leptos::prelude::provide_context(mapbox_token_for_context.clone());
            },
            move || app::shell(leptos_options.clone()),
        )
        .route("/robots.txt", get(crate::pages::legal::robots_txt))
        .route("/sitemap.xml", get(crate::pages::legal::sitemap_xml))
        .route("/favicon.ico", get(crate::pages::legal::favicon))
        .route("/favicon.svg", get(crate::pages::legal::favicon))
        .merge(me_route)
        .route(
            "/oidc",
            axum::routing::any(handle_oidc_redirect::<EmptyAdditionalClaims, SessionWrapper>),
        )
        .route("/logout", get(crate::session::logout))
        .merge(caldav_routes)
        .layer(oidc_auth_service)
        .layer(axum::Extension(app_config))
        .with_state(state)
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

