//! DEPRECATED — the webview's private JSON event API.
//!
//! These four endpoints exist only because the webview was written before
//! the CalDAV write path was finished, and they now duplicate it: each one
//! has an exact CalDAV equivalent, against the same calendar, with the same
//! permission check underneath.
//!
//! | this                                              | use instead                          |
//! |---------------------------------------------------|--------------------------------------|
//! | `GET    /app/{public_id}/api/events`              | `GET    /cal/{public_id}`            |
//! | `POST   /app/{public_id}/api/events`              | `PUT    /cal/{public_id}/{uid}.ics`  |
//! | `PATCH  /app/{public_id}/api/events/{event_id}`   | `PUT    /cal/{public_id}/{uid}.ics`  |
//! | `DELETE /app/{public_id}/api/events/{event_id}`   | `DELETE /cal/{public_id}/{uid}.ics`  |
//!
//! The plan is for the web UI to speak CalDAV like every other client, so
//! there is one write path to reason about rather than two that have to be
//! kept in step. Two paths is not a theoretical cost: the duplicate-page race
//! found in the CalDAV PUT handler (five concurrent PUTs of one UID create
//! five Notion pages) has to be fixed once per path, and a fix applied to
//! only one of them looks like a fix.
//!
//! Marked with RFC 8594 `Deprecation` and `Link` headers by the middleware in
//! `create_app`, and counted as `notioncal.deprecated.api.calls` so the
//! removal decision rests on whether anything still calls them rather than on
//! anyone's memory.
//!
//! Still fully supported until the webview migrates. Nothing here is a stub.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::session::require_owned_calendar;
use crate::AppState;
use app::webview::labels_for;

/// Deprecated: `GET /cal/{public_id}` returns the same events as iCalendar.
pub async fn handle_list_events(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };

    let pages = state.pages_for_calendar(cal.id).await;
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

/// Deprecated: `PUT /cal/{public_id}/{uid}.ics` with an iCalendar body.
pub async fn handle_create_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
        let l = labels_for();
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
            &cal.column_mapping,
        )
        .await
    {
        Ok(page_id) => {
            state
                .log_sync(cal.id, "webview", "create", "", &page_id, "ok", "")
                .await;
                if !state.refresh_single_page(&cal, &page_id).await {
                    state.refresh_by_data_source(&cal.data_source_id).await;
                }
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

/// Deprecated: `PUT /cal/{public_id}/{uid}.ics` — CalDAV has no separate
/// create and update; a PUT to an existing UID is the edit.
pub async fn handle_update_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path((public_id, event_id)): Path<(String, String)>,
    Json(body): Json<UpdateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
        let l = labels_for();
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
            &cal.column_mapping,
        )
        .await
    {
        Ok(()) => {
            state
                .log_sync(cal.id, "webview", "update", &event_id, &event_id, "ok", "")
                .await;
                if !state.refresh_single_page(&cal, &event_id).await {
                    state.refresh_by_data_source(&cal.data_source_id).await;
                }
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

/// Deprecated: `DELETE /cal/{public_id}/{uid}.ics`.
pub async fn handle_delete_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path((public_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
                if !state.refresh_single_page(&cal, &event_id).await {
                    state.refresh_by_data_source(&cal.data_source_id).await;
                }
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
