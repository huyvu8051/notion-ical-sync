use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::pages::webview::labels_for;
use crate::session::require_owned_calendar;
use crate::AppState;

pub async fn handle_list_events(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };

    let cache = state.cache.read().await;
    let pages = cache.get(&cal.database_id).cloned().unwrap_or_default();
    let events: Vec<_> = pages
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "title": p.title,
                "start": p.start,
                "end": p.end,
                "notionUrl": p.url,
                "location": p.location,
                "notes": p.notes,
                "priority": p.priority,
                "busy": p.busy,
                "reminderMinutes": p.reminder_minutes,
                "travelMinutes": p.travel_minutes,
                "repeatRule": p.repeat_rule,
                "attendees": p.attendees,
            })
        })
        .collect();
    Json(events).into_response()
}

#[derive(Deserialize)]
pub struct CreateEventBody {
    title: String,
    start: String,
    end: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    busy: Option<bool>,
    #[serde(default)]
    reminder_minutes: Option<i64>,
    #[serde(default)]
    travel_minutes: Option<i64>,
}

pub async fn handle_create_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
    lang: crate::i18n::Lang,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    if crate::billing::enforce_quota(&state, cal.user_id)
        .await
        .is_err()
    {
        state
            .log_sync(
                cal.id,
                "webview",
                "create",
                "",
                "",
                "error",
                "daily quota exceeded",
            )
            .await;
        let l = labels_for(lang);
        return (StatusCode::TOO_MANY_REQUESTS, l.alert_quota_exceeded).into_response();
    }
    let extra = crate::caldav::ExtraEventFields {
        location: body.location.as_deref(),
        notes: body.notes.as_deref(),
        priority: body.priority,
        busy: body.busy,
        reminder_minutes: body.reminder_minutes,
        travel_minutes: body.travel_minutes,
    };
    match state
        .notion_create_event(
            &cal.data_source_id,
            &cal.date_property,
            &cal.notion_access_token,
            &body.title,
            &body.start,
            body.end.as_deref(),
            &extra,
        )
        .await
    {
        Ok(page_id) => {
            state
                .log_sync(cal.id, "webview", "create", "", &page_id, "ok", "")
                .await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "id": page_id })),
            )
                .into_response()
        }
        Err(e) => {
            error!("webview create event failed: {}", e);
            state
                .log_sync(cal.id, "webview", "create", "", "", "error", &e)
                .await;
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateEventBody {
    title: Option<String>,
    start: Option<String>,
    #[serde(default, deserialize_with = "deserialize_field_present_but_maybe_null")]
    end: Option<Option<String>>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    busy: Option<bool>,
    #[serde(default)]
    reminder_minutes: Option<i64>,
    #[serde(default)]
    travel_minutes: Option<i64>,
}

fn deserialize_field_present_but_maybe_null<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

pub async fn handle_update_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path((public_id, event_id)): Path<(String, String)>,
    lang: crate::i18n::Lang,
    Json(body): Json<UpdateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    if crate::billing::enforce_quota(&state, cal.user_id)
        .await
        .is_err()
    {
        state
            .log_sync(
                cal.id,
                "webview",
                "update",
                &event_id,
                "",
                "error",
                "daily quota exceeded",
            )
            .await;
        let l = labels_for(lang);
        return (StatusCode::TOO_MANY_REQUESTS, l.alert_quota_exceeded).into_response();
    }
    let extra = crate::caldav::ExtraEventFields {
        location: body.location.as_deref(),
        notes: body.notes.as_deref(),
        priority: body.priority,
        busy: body.busy,
        reminder_minutes: body.reminder_minutes,
        travel_minutes: body.travel_minutes,
    };
    match state
        .notion_update_event(
            &event_id,
            &cal.data_source_id,
            &cal.date_property,
            &cal.notion_access_token,
            body.title.as_deref(),
            body.start.as_deref(),
            body.end.as_ref().map(|o| o.as_deref()),
            &extra,
        )
        .await
    {
        Ok(()) => {
            state
                .log_sync(cal.id, "webview", "update", &event_id, &event_id, "ok", "")
                .await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview update event failed: {}", e);
            state
                .log_sync(
                    cal.id, "webview", "update", &event_id, &event_id, "error", &e,
                )
                .await;
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

pub async fn handle_delete_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path((public_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    match state
        .notion_delete_event(&event_id, &cal.notion_access_token)
        .await
    {
        Ok(()) => {
            state
                .log_sync(cal.id, "webview", "delete", &event_id, &event_id, "ok", "")
                .await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview delete event failed: {}", e);
            state
                .log_sync(
                    cal.id, "webview", "delete", &event_id, &event_id, "error", &e,
                )
                .await;
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}
