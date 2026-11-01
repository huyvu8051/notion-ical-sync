use std::collections::HashMap;

use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use tracing::{error, info, warn};

use crate::crypto::{generate_token, hash_password};
use crate::error_page::{error_page, OauthError};
use crate::pages::connect_notion::NOTION_VERSION;
use crate::session::find_or_create_user;
use crate::caldav::DatabaseParent;
use crate::AppState;
use app::column_mapping::{
    mapping_for_created_schema, schema_from_pairs, ColumnMapping, Field, CREATED_DATE_PROPERTY,
};

struct DatabaseCandidate {
    database_id: String,
    data_source_id: String,
    title: String,
    /// Every property of the data source as `(name, notion type)`, kept so the
    /// user can pick which one backs each calendar field. Previously only the
    /// first date-typed name survived and the rest was discarded.
    properties: Vec<(String, String)>,
}

impl DatabaseCandidate {
    fn date_properties(&self) -> Vec<&str> {
        self.properties
            .iter()
            .filter(|(_, notion_type)| notion_type == "date")
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

async fn list_syncable_databases(
    client: &reqwest::Client,
    api_base_url: &str,
    token: &str,
) -> Result<Vec<DatabaseCandidate>, String> {
    let resp = client
        .post(format!("{api_base_url}/v1/search"))
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({ "filter": { "value": "data_source", "property": "object" } }))
        .send()
        .await
        .map_err(|e| format!("search request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Notion search error {status}: {txt}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;
    let results = body
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    let mut candidates = Vec::new();
    for ds in results {
        let Some(data_source_id) = ds.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(database_id) = ds
            .get("parent")
            .and_then(|p| p.get("database_id"))
            .and_then(|id| id.as_str())
        else {
            continue;
        };

        let title = ds
            .get("title")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("plain_text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)")
            .to_string();
        let mut properties: Vec<(String, String)> = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|props| {
                props
                    .iter()
                    .filter_map(|(name, def)| {
                        let notion_type = def.get("type")?.as_str()?;
                        Some((name.clone(), notion_type.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // serde_json has no preserve_order here, so map iteration is
        // alphabetical rather than Notion's column order. Sort explicitly so
        // the dropdowns at least have a stable, predictable order.
        properties.sort_by_key(|(name, _)| name.to_lowercase());

        candidates.push(DatabaseCandidate {
            database_id: database_id.to_string(),
            data_source_id: data_source_id.to_string(),
            title,
            properties,
        });
    }

    info!(count = candidates.len(), "listed syncable notion databases");
    Ok(candidates)
}

async fn connection_token_for_user(
    state: &AppState,
    connection_id: i64,
    user_id: i64,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT notion_access_token FROM notion_connections WHERE id = $1 AND user_id = $2",
    )
    .bind(connection_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None)
}

struct CreateCalendarsForm {
    connection_id: i64,
    db_ids: Vec<String>,
    /// Chosen date property per database, from `date[<database_id>]`.
    date_properties: HashMap<String, String>,
    /// Chosen property per calendar field per database, from
    /// `map[<database_id>][<field key>]`.
    mappings: HashMap<String, ColumnMapping>,
}

/// Split `prefix[a][b]` into `("a", "b")`. Returns `None` for any other shape,
/// so a malformed or unexpected field name is ignored rather than guessed at.
fn parse_bracketed<'a>(key: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = key.strip_prefix(prefix)?.strip_prefix('[')?;
    let (first, rest) = rest.split_once(']')?;
    let second = rest.strip_prefix('[')?.strip_suffix(']')?;
    Some((first, second))
}

impl CreateCalendarsForm {
    fn parse(body: &str) -> Option<Self> {
        let mut connection_id = None;
        let mut db_ids = Vec::new();
        let mut date_properties: HashMap<String, String> = HashMap::new();
        let mut mappings: HashMap<String, ColumnMapping> = HashMap::new();

        for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
            match key.as_ref() {
                "connection_id" => connection_id = value.parse::<i64>().ok(),
                "db_ids" => db_ids.push(value.into_owned()),
                // `date[<database_id>]`
                k if k.starts_with("date[") && k.ends_with(']') => {
                    if let Some(db_id) = k
                        .strip_prefix("date[")
                        .and_then(|rest| rest.strip_suffix(']'))
                    {
                        if !value.is_empty() {
                            date_properties.insert(db_id.to_string(), value.into_owned());
                        }
                    }
                }
                // `map[<database_id>][<field key>]`
                k if k.starts_with("map[") => {
                    let Some((db_id, field_key)) = parse_bracketed(k, "map") else {
                        warn!(key = k, "ignoring malformed mapping form field");
                        continue;
                    };
                    let Some(field) = Field::from_key(field_key) else {
                        warn!(key = k, field_key, "ignoring unknown mapping field");
                        continue;
                    };
                    let entry = mappings.entry(db_id.to_string()).or_default();
                    // The user saw every dropdown, so an absent choice is a
                    // deliberate "none" rather than "fall back to the guess".
                    entry.configured = true;
                    entry.set(field, Some(value.into_owned()));
                }
                _ => {}
            }
        }

        Some(Self {
            connection_id: connection_id?,
            db_ids,
            date_properties,
            mappings,
        })
    }
}

/// The outcome of trying to turn one Notion database into a calendar.
enum CalendarInsert {
    /// Freshly minted CalDAV credentials, which are shown to the user once and
    /// never again.
    Created { username: String, password: String },
    /// They already sync this database; nothing was written.
    AlreadyConnected,
    Failed,
}

/// Inserts one calendar row and mints its CalDAV credentials.
///
/// Shared by both ways a calendar comes into being -- the user picking a
/// database they already had, and the app building one for them -- so the
/// credential minting and the already-connected case cannot drift apart
/// between the two.
#[allow(clippy::too_many_arguments)]
async fn insert_calendar(
    state: &AppState,
    user_id: i64,
    connection_id: i64,
    database_id: &str,
    data_source_id: &str,
    date_property: &str,
    display_name: &str,
    mapping: &ColumnMapping,
) -> CalendarInsert {
    let public_id = uuid::Uuid::new_v4().to_string();
    let caldav_username = format!("cal_{}", generate_token(12));
    let caldav_password = generate_token(24);
    let password_hash = match hash_password(&caldav_password) {
        Ok(h) => h,
        Err(e) => {
            error!("failed to hash caldav password: {}", e);
            return CalendarInsert::Failed;
        }
    };
    let result = sqlx::query(
        "INSERT INTO calendars (user_id, notion_connection_id, database_id, public_id, data_source_id, date_property, display_name, caldav_username, caldav_password_hash, column_mapping)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (user_id, database_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(connection_id)
    .bind(database_id)
    .bind(&public_id)
    .bind(data_source_id)
    .bind(date_property)
    .bind(display_name)
    .bind(&caldav_username)
    .bind(&password_hash)
    .bind(sqlx::types::Json(mapping))
    .execute(&state.db)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => CalendarInsert::Created {
            username: caldav_username,
            password: caldav_password,
        },
        Ok(_) => CalendarInsert::AlreadyConnected,
        Err(e) => {
            error!("failed to insert calendar {}: {}", database_id, e);
            CalendarInsert::Failed
        }
    }
}

pub async fn create_calendars(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let body = String::from_utf8_lossy(&body);
    let Some(form) = CreateCalendarsForm::parse(&body) else {
        return error_page(OauthError::InvalidRequest);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email).await {
        Ok(id) => id,
        Err(_) => return error_page(OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, form.connection_id, user_id).await
    else {
        return error_page(OauthError::ConnectionNotFound);
    };

    if form.db_ids.is_empty() {
        return Redirect::to("/me").into_response();
    }

    let candidates =
        match list_syncable_databases(&state.client, &state.notion_api_base_url, &access_token)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                error!("failed to list notion databases: {}", e);
                return error_page(OauthError::FailedToListDatabases);
            }
        };

    let mut newly_created_calendar_credentials: Vec<(String, String, String)> = Vec::new();
    let mut already_connected_database_titles: Vec<String> = Vec::new();

    for db_id in &form.db_ids {
        let Some(candidate) = candidates.iter().find(|c| &c.database_id == db_id) else {
            warn!(
                db_id,
                "posted database is not in the user's Notion search results, skipping"
            );
            continue;
        };

        // Never trust the posted column names: check them against the schema
        // we just fetched, not against what the browser claimed.
        let schema = schema_from_pairs(&candidate.properties);
        let available_dates = candidate.date_properties();
        let Some(date_property) = form
            .date_properties
            .get(db_id)
            .filter(|chosen| available_dates.iter().any(|d| d == chosen))
            .cloned()
            .or_else(|| available_dates.first().map(|d| d.to_string()))
        else {
            warn!(
                db_id,
                title = %candidate.title,
                "skipping database: it has no date property to sync on"
            );
            continue;
        };

        let mut mapping = form.mappings.get(db_id).cloned().unwrap_or_else(|| {
            // No mapping posted at all (older cached page, or JS-free submit):
            // fall back to the same guesses the UI would have preselected.
            ColumnMapping::guess_from_schema(&schema)
        });
        mapping.retain_valid(&schema);
        info!(
            db_id,
            title = %candidate.title,
            date_property = %date_property,
            title_property = mapping.name_for(Field::Title).unwrap_or("<none>"),
            "creating calendar with column mapping"
        );

        match insert_calendar(
            &state,
            user_id,
            form.connection_id,
            &candidate.database_id,
            &candidate.data_source_id,
            &date_property,
            &candidate.title,
            &mapping,
        )
        .await
        {
            CalendarInsert::Created { username, password } => newly_created_calendar_credentials
                .push((candidate.title.clone(), username, password)),
            CalendarInsert::AlreadyConnected => {
                already_connected_database_titles.push(candidate.title.clone())
            }
            CalendarInsert::Failed => {}
        }
    }

    if !newly_created_calendar_credentials.is_empty() {
        if let Err(e) = session
            .insert(
                "new_calendar_credentials",
                &newly_created_calendar_credentials,
            )
            .await
        {
            error!("failed to stash new calendar credentials in session: {}", e);
        }
        state.refresh_all().await;
    }

    if !already_connected_database_titles.is_empty() {
        if let Err(e) = session
            .insert(
                "calendar_connect_errors",
                &already_connected_database_titles,
            )
            .await
        {
            error!("failed to stash calendar connect errors in session: {}", e);
        }
    }

    Redirect::to("/me").into_response()
}


/// The name the app gives a calendar database it builds.
pub const CREATED_DATABASE_TITLE: &str = "Calendar";

/// A page the connection may write to, for when a workspace-level create is
/// refused.
///
/// First match wins and the choice is arbitrary, which is the point: this only
/// runs on the path where the better option already failed, and any granted
/// page is a better home for the database than no database at all. The user
/// can move it afterwards; Notion keeps the ids.
async fn first_grantable_page(
    client: &reqwest::Client,
    api_base_url: &str,
    token: &str,
) -> Option<String> {
    let resp = client
        .post(format!("{api_base_url}/v1/search"))
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({
            "filter": { "value": "page", "property": "object" },
            "page_size": 1,
        }))
        .send()
        .await
        .ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("results")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(|s| s.to_string())
}

/// Builds a correctly-shaped calendar database for a user who has none.
///
/// Before this, a Notion workspace with no suitable database left the pick
/// page with nothing to offer but a link back to Notion and the implicit
/// instruction to go build the right columns by hand -- a schema the app had
/// never written down anywhere the user could read.
///
/// The calendar lands fully configured: the app chose every column name, so
/// the mapping it stores is exact and the mapping step is skipped rather than
/// guessed at.
pub async fn create_standard_database(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let body = String::from_utf8_lossy(&body);
    let Some(connection_id) = url::form_urlencoded::parse(body.as_bytes())
        .find(|(key, _)| key == "connection_id")
        .and_then(|(_, value)| value.parse::<i64>().ok())
    else {
        return error_page(OauthError::InvalidRequest);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email).await {
        Ok(id) => id,
        Err(_) => return error_page(OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, connection_id, user_id).await else {
        return error_page(OauthError::ConnectionNotFound);
    };

    // Workspace first, and only hunt for a parent page if that is refused --
    // the search costs a round trip that the working path should not pay.
    let created = match state
        .notion_create_calendar_database(
            &access_token,
            CREATED_DATABASE_TITLE,
            DatabaseParent::Workspace,
        )
        .await
    {
        Ok(created) => created,
        Err(workspace_err) => {
            let Some(page_id) =
                first_grantable_page(&state.client, &state.notion_api_base_url, &access_token).await
            else {
                error!(
                    "failed to create a calendar database in notion: {}",
                    workspace_err
                );
                return error_page(OauthError::FailedToCreateDatabase);
            };
            warn!(
                error = %workspace_err,
                page_id,
                "workspace-parent create refused, retrying under a granted page"
            );
            match state
                .notion_create_calendar_database(
                    &access_token,
                    CREATED_DATABASE_TITLE,
                    DatabaseParent::Page(&page_id),
                )
                .await
            {
                Ok(created) => created,
                Err(e) => {
                    error!("failed to create a calendar database under a page: {}", e);
                    return error_page(OauthError::FailedToCreateDatabase);
                }
            }
        }
    };

    info!(
        database_id = %created.database_id,
        data_source_id = %created.data_source_id,
        "created a standard calendar database"
    );

    let mapping = mapping_for_created_schema();
    match insert_calendar(
        &state,
        user_id,
        connection_id,
        &created.database_id,
        &created.data_source_id,
        CREATED_DATE_PROPERTY,
        CREATED_DATABASE_TITLE,
        &mapping,
    )
    .await
    {
        CalendarInsert::Created { username, password } => {
            let credentials = vec![(CREATED_DATABASE_TITLE.to_string(), username, password)];
            if let Err(e) = session.insert("new_calendar_credentials", &credentials).await {
                error!("failed to stash new calendar credentials in session: {}", e);
            }
            state.refresh_all().await;
        }
        // The database was just created, so its id cannot already be a
        // calendar of theirs; reaching here means the insert lost a race with
        // a double submit, and the duplicate row is the one to drop.
        CalendarInsert::AlreadyConnected => {
            warn!(
                database_id = %created.database_id,
                "freshly created database was already a calendar, ignoring the duplicate"
            );
        }
        CalendarInsert::Failed => return error_page(OauthError::FailedToSaveConnection),
    }

    Redirect::to("/me").into_response()
}
