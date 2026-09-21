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
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::{error, info};

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

fn extract_busy(props: &serde_json::Value) -> Option<bool> {
    if let Some(prop) = find_property_case_insensitive(props, "Busy") {
        if prop.get("type").and_then(|t| t.as_str()) == Some("checkbox") {
            if let Some(b) = prop.get("checkbox").and_then(|v| v.as_bool()) {
                return Some(b);
            }
        }
    }
    let show_as = extract_select_name(props, "Show As")?;
    Some(show_as.eq_ignore_ascii_case("busy"))
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
    pub cache: Arc<RwLock<HashMap<String, Vec<PageInfo>>>>,
    pub caldav_allow_writes: CaldavAllowWrites,
    pub webhook_secret: Option<String>,
    pub notion_oauth: Option<crate::pages::connect_notion::NotionOAuthConfig>,
    pub notion_api_base_url: String,
    pub mapbox_token: Option<String>,
    pub password_enc_key: Option<[u8; 32]>,
    pub stripe: Option<crate::billing::StripeConfig>,
    pub email: Option<crate::email::EmailConfig>,
    pub admin_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotionQueryResponse {
    results: Vec<serde_json::Value>,
}

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
}

#[derive(Debug, Clone)]
pub struct AuthenticatedCaldavUser {
    pub user_id: i64,
    pub username: String,
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
        password_enc_key: Option<[u8; 32]>,
        stripe: Option<crate::billing::StripeConfig>,
        email: Option<crate::email::EmailConfig>,
        admin_secret: Option<String>,
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
            notion_api_base_url,
            mapbox_token,
            password_enc_key,
            stripe,
            email,
            admin_secret,
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

    pub async fn verify_caldav_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Option<(i64, String)> {
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
        argon2::Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .ok()?;
        Some((row.user_id, row.public_id))
    }

    pub async fn refresh_for_user(&self, user_id: i64) {
        for cal in self.calendars_for_user(user_id).await {
            match self
                .refresh_db(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                )
                .await
            {
                Ok(pages) => {
                    info!("DB {} synced: {} events", cal.database_id, pages.len());
                    self.cache.write().await.insert(cal.database_id, pages);
                }
                Err(e) => error!("DB {} refresh failed: {}", cal.database_id, e),
            }
        }
    }

    pub async fn refresh_db(
        &self,
        ds_id: &str,
        date_property: &str,
        notion_token: &str,
    ) -> Result<Vec<PageInfo>, String> {
        let url = format!("{}/v1/data_sources/{}/query", self.notion_api_base_url, ds_id);

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

            let location = extract_text_or_url_property(props, "Location");
            let notes = extract_text_or_url_property(props, "Notes");
            let priority = extract_select_name(props, "Priority").and_then(|s| map_priority(&s));
            let busy = extract_busy(props);
            let reminder_minutes = extract_number_property(props, "Reminder");
            let travel_minutes = extract_number_property(props, "Travel time");
            let repeat_rule =
                extract_select_name(props, "Repeat").and_then(|s| map_repeat_to_rrule(&s));
            let attendees = extract_attendees(props, "Attendees");

            events.push(PageInfo {
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
            });
        }

        Ok(events)
    }

    pub async fn refresh_all(&self) {
        let calendars = self.all_calendars().await;
        let mut seen = std::collections::HashSet::new();
        let mut cache = self.cache.write().await;
        for cal in calendars {
            if !seen.insert(cal.database_id.clone()) {
                continue;
            }
            match self
                .refresh_db(
                    &cal.data_source_id,
                    &cal.date_property,
                    &cal.notion_access_token,
                )
                .await
            {
                Ok(pages) => {
                    info!("DB {} synced: {} events", cal.database_id, pages.len());
                    cache.insert(cal.database_id, pages);
                }
                Err(e) => error!("DB {} refresh failed: {}", cal.database_id, e),
            }
        }
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
            )
            .await
        {
            Ok(pages) => {
                info!(
                    "DB {} synced via webhook: {} events",
                    cal.database_id,
                    pages.len()
                );
                self.cache.write().await.insert(cal.database_id, pages);
            }
            Err(e) => error!(
                "DB {} webhook-triggered refresh failed: {}",
                cal.database_id, e
            ),
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

    async fn get_data_source_properties(
        &self,
        data_source_id: &str,
        notion_token: &str,
    ) -> HashMap<String, (String, String)> {
        let url = format!("{}/v1/data_sources/{}", self.notion_api_base_url, data_source_id);
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

fn optional_event_properties(
    schema: &HashMap<String, (String, String)>,
    fields: &ExtraEventFields,
) -> serde_json::Map<String, serde_json::Value> {
    let mut properties = serde_json::Map::new();

    if let Some(location) = fields.location {
        if let Some((name, ptype)) = schema.get("location") {
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
        if let Some((name, ptype)) = schema.get("notes") {
            if ptype == "rich_text" {
                properties.insert(
                    name.clone(),
                    serde_json::json!({ "rich_text": [{ "text": { "content": notes } }] }),
                );
            }
        }
    }
    if let Some(priority) = fields.priority {
        if let Some((name, ptype)) = schema.get("priority") {
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
        if let Some((name, ptype)) = schema.get("busy") {
            if ptype == "checkbox" {
                properties.insert(name.clone(), serde_json::json!({ "checkbox": busy }));
            }
        } else if let Some((name, ptype)) = schema.get("show as") {
            if ptype == "select" {
                properties.insert(
                    name.clone(),
                    serde_json::json!({ "select": { "name": if busy { "Busy" } else { "Free" } } }),
                );
            }
        }
    }
    if let Some(minutes) = fields.reminder_minutes {
        if let Some((name, ptype)) = schema.get("reminder") {
            if ptype == "number" {
                properties.insert(name.clone(), serde_json::json!({ "number": minutes }));
            }
        }
    }
    if let Some(minutes) = fields.travel_minutes {
        if let Some((name, ptype)) = schema.get("travel time") {
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
        let schema = self
            .get_data_source_properties(data_source_id, notion_token)
            .await;
        properties.extend(optional_event_properties(&schema, extra));

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
        let schema = self
            .get_data_source_properties(data_source_id, notion_token)
            .await;
        properties.extend(optional_event_properties(&schema, extra));

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
    db_id: &str,
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
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        let body = build_ics(&public_id, &name, &pages);
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
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        let body = build_report_response(&public_id, &prefix, &name, &pages);
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
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
        if let Some(page) = pages.iter().find(|p| matches_id(&p.id, &event_id_clean)) {
            let body = build_ics(&public_id, &name, std::slice::from_ref(page));
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
        let cache = state.cache.read().await;
        let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
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
        let existing_id = {
            let cache = state.cache.read().await;
            cache.get(&cal.database_id).and_then(|pages| {
                pages
                    .iter()
                    .find(|p| matches_id(&p.id, &event_id_clean))
                    .map(|p| p.id.clone())
            })
        };
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
                state.refresh_by_data_source(&cal.data_source_id).await;
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
        let existing_id = {
            let cache = state.cache.read().await;
            cache.get(&cal.database_id).and_then(|pages| {
                pages
                    .iter()
                    .find(|p| matches_id(&p.id, &event_id_clean))
                    .map(|p| p.id.clone())
            })
        };
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
                state.refresh_by_data_source(&cal.data_source_id).await;
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
            return crate::pages::landing::landing_page(
                crate::i18n::Lang::detect(&headers),
                synthetic_request,
            )
            .await
            .into_response();
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

        let cache = state.cache.read().await;
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

    let Some((user_id, auth_public_id)) =
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
    request
        .extensions_mut()
        .insert(AuthenticatedCaldavUser { user_id, username });

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
    app_config: crate::session::AppConfig,
) -> Router {
    use crate::session::SessionWrapper;
    use axum::error_handling::HandleErrorLayer;
    use axum_oidc::{
        error::MiddlewareError, handle_oidc_redirect, EmptyAdditionalClaims, OidcAuthLayer,
        OidcLoginLayer,
    };
    use tower::ServiceBuilder;

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

    let me_route = Router::new()
        .route("/me", get(crate::pages::me::me))
        .route(
            "/connect/notion",
            get(crate::pages::connect_notion::connect_notion_page),
        )
        .route(
            "/connect/notion/start",
            get(crate::pages::connect_notion::connect_notion_start),
        )
        .route(
            "/connect/notion/databases",
            get(crate::pages::pick_databases::pick_databases_page)
                .post(crate::pages::pick_databases::create_calendars),
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
            "/me/calendars/{public_id}/reveal-password",
            post(crate::pages::me::reveal_password),
        )
        .route(
            "/me/calendars/{public_id}/regenerate-password",
            post(crate::pages::me::regenerate_password),
        )
        .route(
            "/me/calendars/{public_id}/log",
            get(crate::pages::sync_log::sync_log_page),
        )
        .route(
            "/app",
            get(|| async { axum::response::Redirect::to("/me") }),
        )
        .route(
            "/app/{public_id}",
            get(crate::pages::webview::handle_webview_page),
        )
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
            post(crate::billing::handle_stripe_webhook),
        )
        .route("/admin/reset-billing", post(crate::billing::reset_billing))
        .nest_service(
            "/pkg",
            tower::ServiceBuilder::new()
                .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-cache"),
                ))
                .service(tower_http::services::ServeDir::new("pkg")),
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
        .route("/privacy", get(crate::pages::legal::privacy_policy_page))
        .route("/terms", get(crate::pages::legal::terms_of_service_page))
        .route("/robots.txt", get(crate::pages::legal::robots_txt))
        .route("/sitemap.xml", get(crate::pages::legal::sitemap_xml))
        .route("/favicon.ico", get(crate::pages::legal::favicon))
        .route("/favicon.svg", get(crate::pages::legal::favicon))
        .route("/lang/{code}", get(crate::i18n::set_lang))
        .route("/dev/leptos-check", get(leptos_axum::render_app_to_stream(app::Shell)))
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

#[cfg(test)]
mod new_field_tests {
    use super::*;

    fn base_page() -> PageInfo {
        PageInfo {
            id: "abc123".to_string(),
            title: "Test event".to_string(),
            start: "2026-01-01T10:00:00.000Z".to_string(),
            end: None,
            url: "https://notion.so/abc123".to_string(),
            last_edited: "2026-01-01T00:00:00.000Z".to_string(),
            location: None,
            notes: None,
            priority: None,
            busy: None,
            reminder_minutes: None,
            travel_minutes: None,
            repeat_rule: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn build_ics_omits_all_new_fields_when_absent() {
        let ics = build_ics("db", "Cal", &[base_page()]);
        assert!(ics.contains("URL:https://notion.so/abc123"));
        assert!(!ics.contains("DESCRIPTION:"));
        assert!(!ics.contains("LOCATION:"));
        assert!(!ics.contains("PRIORITY:"));
        assert!(!ics.contains("TRANSP:"));
        assert!(!ics.contains("RRULE:"));
        assert!(!ics.contains("ATTENDEE"));
        assert!(!ics.contains("VALARM"));
    }

    #[test]
    fn build_ics_emits_every_new_field_when_present() {
        let mut page = base_page();
        page.notes = Some("Bring the slides".to_string());
        page.location = Some("Room 4B".to_string());
        page.priority = Some(1);
        page.busy = Some(true);
        page.reminder_minutes = Some(15);
        page.travel_minutes = Some(20);
        page.repeat_rule = Some("FREQ=WEEKLY".to_string());
        page.attendees = vec!["a@example.com".to_string(), "b@example.com".to_string()];

        let ics = build_ics("db", "Cal", &[page]);
        assert!(ics.contains("DESCRIPTION:Bring the slides"));
        assert!(ics.contains("LOCATION:Room 4B"));
        assert!(ics.contains("PRIORITY:1"));
        assert!(ics.contains("TRANSP:OPAQUE"));
        assert!(ics.contains("RRULE:FREQ=WEEKLY"));
        assert!(ics.contains("ATTENDEE;CN=a@example.com:mailto:a@example.com"));
        assert!(ics.contains("ATTENDEE;CN=b@example.com:mailto:b@example.com"));
        assert!(ics.contains("X-APPLE-TRAVEL-DURATION;VALUE=DURATION:PT20M"));
        assert!(ics.contains("BEGIN:VALARM"));
        assert!(ics.contains("TRIGGER:-PT15M"));
        assert!(ics.contains("END:VALARM"));
    }

    #[test]
    fn build_ics_transp_transparent_when_not_busy() {
        let mut page = base_page();
        page.busy = Some(false);
        let ics = build_ics("db", "Cal", &[page]);
        assert!(ics.contains("TRANSP:TRANSPARENT"));
    }

    #[test]
    fn priority_mapping_matches_ical_semantics() {
        assert_eq!(map_priority("High"), Some(1));
        assert_eq!(map_priority("medium"), Some(5));
        assert_eq!(map_priority("LOW"), Some(9));
        assert_eq!(map_priority("Urgent"), None);
    }

    #[test]
    fn repeat_mapping_matches_expected_rrule_freq() {
        assert_eq!(
            map_repeat_to_rrule("Weekly"),
            Some("FREQ=WEEKLY".to_string())
        );
        assert_eq!(map_repeat_to_rrule("none"), None);
    }

    #[test]
    fn extract_attendees_splits_rich_text_list() {
        let props = serde_json::json!({
            "Attendees": {
                "type": "rich_text",
                "rich_text": [{"plain_text": "a@x.com, b@x.com; c@x.com"}]
            }
        });
        assert_eq!(
            extract_attendees(&props, "Attendees"),
            vec!["a@x.com", "b@x.com", "c@x.com"]
        );
    }

    #[test]
    fn extract_busy_prefers_checkbox_over_show_as() {
        let props = serde_json::json!({
            "Busy": {"type": "checkbox", "checkbox": false},
            "Show As": {"type": "select", "select": {"name": "Busy"}}
        });
        assert_eq!(extract_busy(&props), Some(false));
    }

    #[test]
    fn find_property_case_insensitive_matches_any_casing() {
        let props = serde_json::json!({"lOcAtIoN": {"type": "url", "url": "https://x.com"}});
        assert_eq!(
            extract_text_or_url_property(&props, "Location"),
            Some("https://x.com".to_string())
        );
    }

    #[test]
    fn parse_ics_extracts_location_notes_priority_transp_and_valarm() {
        let ics = "BEGIN:VEVENT\r\n\
UID:evt-1\r\n\
SUMMARY:Standup\r\n\
DTSTART:20260101T100000Z\r\n\
LOCATION:Room 4B\r\n\
DESCRIPTION:Bring the slides\r\n\
PRIORITY:1\r\n\
TRANSP:TRANSPARENT\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
TRIGGER:-PT15M\r\n\
END:VALARM\r\n\
END:VEVENT\r\n";
        let page = parse_ics_to_page_info(ics, "fallback-id");
        assert_eq!(page.id, "evt-1");
        assert_eq!(page.title, "Standup");
        assert_eq!(page.location, Some("Room 4B".to_string()));
        assert_eq!(page.notes, Some("Bring the slides".to_string()));
        assert_eq!(page.priority, Some(1));
        assert_eq!(page.busy, Some(false));
        assert_eq!(page.reminder_minutes, Some(15));
        assert_eq!(page.url, "");
    }

    #[test]
    fn parse_ics_omits_new_fields_when_absent() {
        let ics = "BEGIN:VEVENT\r\nUID:evt-2\r\nSUMMARY:Plain\r\nDTSTART:20260101T100000Z\r\nEND:VEVENT\r\n";
        let page = parse_ics_to_page_info(ics, "fallback-id");
        assert_eq!(page.location, None);
        assert_eq!(page.notes, None);
        assert_eq!(page.priority, None);
        assert_eq!(page.busy, None);
        assert_eq!(page.reminder_minutes, None);
    }

    #[test]
    fn priority_number_to_select_name_covers_full_1_to_9_range() {
        assert_eq!(priority_number_to_select_name(1), Some("High"));
        assert_eq!(priority_number_to_select_name(3), Some("High"));
        assert_eq!(priority_number_to_select_name(4), Some("Medium"));
        assert_eq!(priority_number_to_select_name(6), Some("Medium"));
        assert_eq!(priority_number_to_select_name(7), Some("Low"));
        assert_eq!(priority_number_to_select_name(9), Some("Low"));
        assert_eq!(priority_number_to_select_name(0), None);
    }

    #[test]
    fn optional_event_properties_skips_fields_missing_from_schema() {
        let mut schema = HashMap::new();
        schema.insert(
            "location".to_string(),
            ("Location".to_string(), "rich_text".to_string()),
        );
        let fields = ExtraEventFields {
            location: Some("Somewhere"),
            notes: Some("Ignored — no Notes property"),
            priority: Some(1),
            busy: Some(true),
            reminder_minutes: Some(10),
            travel_minutes: Some(5),
        };
        let props = optional_event_properties(&schema, &fields);
        assert_eq!(props.len(), 1);
        assert!(props.contains_key("Location"));
    }
}

#[cfg(test)]
mod ics_dt_tests {
    use super::*;

    #[test]
    fn converts_negative_offset_to_true_utc() {
        assert_eq!(
            ics_dt("2026-09-17T18:00:00.000-06:00"),
            "20260918T000000Z"
        );
    }

    #[test]
    fn converts_positive_offset_to_true_utc() {
        assert_eq!(
            ics_dt("2026-09-17T18:00:00.000+07:00"),
            "20260917T110000Z"
        );
    }

    #[test]
    fn passes_through_already_utc_values_unchanged() {
        assert_eq!(ics_dt("2026-09-17T18:00:00.000Z"), "20260917T180000Z");
    }

    #[test]
    fn formats_date_only_values_as_all_day() {
        assert_eq!(ics_dt("2026-09-17"), ";VALUE=DATE:20260917");
    }

    #[test]
    fn empty_value_yields_empty_string() {
        assert_eq!(ics_dt(""), "");
    }
}
