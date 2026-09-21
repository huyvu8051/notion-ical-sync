use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::crypto::{generate_token, hash_password};
use crate::error_page::{error_page, OauthError};
use crate::pages::connect_notion::NOTION_VERSION;
use crate::session::find_or_create_user;
use crate::AppState;

struct DatabaseCandidate {
    database_id: String,
    data_source_id: String,
    title: String,
    icon_emoji: Option<String>,
    date_property: Option<String>,
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
        let icon_emoji = ds
            .get("icon")
            .and_then(|icon| icon.get("emoji"))
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let date_property = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .and_then(|props| {
                props
                    .iter()
                    .find(|(_, def)| def.get("type").and_then(|t| t.as_str()) == Some("date"))
                    .map(|(name, _)| name.clone())
            });

        candidates.push(DatabaseCandidate {
            database_id: database_id.to_string(),
            data_source_id: data_source_id.to_string(),
            title,
            icon_emoji,
            date_property,
        });
    }

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

#[derive(Debug, Deserialize)]
pub struct DatabasesPageParams {
    connection_id: i64,
}

pub async fn pick_databases_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Query(params): Query<DatabasesPageParams>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return error_page(lang, OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, params.connection_id, user_id).await
    else {
        return error_page(lang, OauthError::ConnectionNotFound);
    };

    let candidates = match list_syncable_databases(&state.client, &state.notion_api_base_url, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page(lang, OauthError::FailedToListDatabases);
        }
    };

    let data = app::pick_databases::PickDatabasesPageData {
        top_nav_html: crate::i18n::top_nav_html(&email, lang, "/connect/notion/databases"),
        connection_id: params.connection_id,
        candidates: candidates
            .into_iter()
            .map(|c| app::pick_databases::CandidateData {
                icon: c.icon_emoji.unwrap_or_else(|| "📄".to_string()),
                title: c.title,
                database_id: c.database_id,
                date_property: c.date_property,
            })
            .collect(),
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::pick_databases::PickDatabasesShell data=data/> }
    });
    handler(request).await
}

struct CreateCalendarsForm {
    connection_id: i64,
    db_ids: Vec<String>,
}

impl CreateCalendarsForm {
    fn parse(body: &str) -> Option<Self> {
        let mut connection_id = None;
        let mut db_ids = Vec::new();
        for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
            match key.as_ref() {
                "connection_id" => connection_id = value.parse::<i64>().ok(),
                "db_ids" => db_ids.push(value.into_owned()),
                _ => {}
            }
        }
        Some(Self {
            connection_id: connection_id?,
            db_ids,
        })
    }
}

pub async fn create_calendars(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let body = String::from_utf8_lossy(&body);
    let Some(form) = CreateCalendarsForm::parse(&body) else {
        return error_page(lang, OauthError::InvalidRequest);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return error_page(lang, OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, form.connection_id, user_id).await
    else {
        return error_page(lang, OauthError::ConnectionNotFound);
    };

    if form.db_ids.is_empty() {
        return Redirect::to("/me").into_response();
    }

    let candidates = match list_syncable_databases(&state.client, &state.notion_api_base_url, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page(lang, OauthError::FailedToListDatabases);
        }
    };

    let mut newly_created_calendar_credentials: Vec<(String, String, String)> = Vec::new();
    let mut already_connected_database_titles: Vec<String> = Vec::new();

    for db_id in &form.db_ids {
        let Some(candidate) = candidates
            .iter()
            .find(|c| &c.database_id == db_id && c.date_property.is_some())
        else {
            continue;
        };
        let date_property = candidate.date_property.clone().expect("checked above");

        let public_id = uuid::Uuid::new_v4().to_string();
        let caldav_username = format!("cal_{}", generate_token(12));
        let caldav_password = generate_token(24);
        let password_hash = match hash_password(&caldav_password) {
            Ok(h) => h,
            Err(e) => {
                error!("failed to hash caldav password: {}", e);
                continue;
            }
        };
        let result = sqlx::query(
            "INSERT INTO calendars (user_id, notion_connection_id, database_id, public_id, data_source_id, date_property, display_name, caldav_username, caldav_password_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (user_id, database_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(form.connection_id)
        .bind(&candidate.database_id)
        .bind(&public_id)
        .bind(&candidate.data_source_id)
        .bind(&date_property)
        .bind(&candidate.title)
        .bind(&caldav_username)
        .bind(&password_hash)
        .execute(&state.db)
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => newly_created_calendar_credentials.push((
                candidate.title.clone(),
                caldav_username,
                caldav_password,
            )),
            Ok(_) => already_connected_database_titles.push(candidate.title.clone()),
            Err(e) => error!("failed to insert calendar {}: {}", candidate.database_id, e),
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
