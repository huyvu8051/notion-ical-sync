use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use leptos::prelude::*;
use serde::Deserialize;
use tracing::error;

use crate::auth::find_or_create_user;
use crate::AppState;

async fn current_user_id(state: &AppState, claims: &OidcClaims<EmptyAdditionalClaims>) -> Result<i64, StatusCode> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    find_or_create_user(&state.db, sub, email).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Every `/app/{db_id}/...` handler needs this same check: the calendar must
/// exist, and must belong to whoever is logged in — otherwise one user could
/// read or edit another user's Notion events just by guessing a database_id.
async fn require_owned_calendar(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
    db_id: &str,
) -> Result<crate::caldav::CalendarRow, StatusCode> {
    let user_id = current_user_id(state, claims).await?;
    match state.calendar_by_db_id(db_id).await {
        Some(cal) if cal.user_id == user_id => Ok(cal),
        Some(_) => Err(StatusCode::FORBIDDEN),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// `/app` — lists the logged-in user's own calendars (by their real Notion
/// name) so you don't have to know/type a database_id by hand.
pub async fn handle_webview_index(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
) -> impl IntoResponse {
    let user_id = match current_user_id(&state, &claims).await {
        Ok(id) => id,
        Err(status) => return status.into_response(),
    };

    let mut items = Vec::new();
    for cal in state.calendars_for_user(user_id).await {
        let name = if cal.display_name.is_empty() {
            state.get_calendar_name(&cal.database_id, &cal.notion_access_token).await
        } else {
            cal.display_name.clone()
        };
        items.push((cal.database_id, name));
    }

    let html = view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <title>"Notion Calendars"</title>
                <style>
                    "body { font-family: sans-serif; margin: 2rem; max-width: 600px; }
                    li { line-height: 2.2; font-size: 1.1rem; }
                    a { color: #2563eb; text-decoration: none; }
                    a:hover { text-decoration: underline; }"
                </style>
            </head>
            <body>
                <h1>"Notion Calendars"</h1>
                <ul>
                    {items
                        .into_iter()
                        .map(|(db_id, name)| {
                            let href = format!("/app/{}", db_id);
                            view! { <li><a href=href>{name}</a></li> }
                        })
                        .collect_view()}
                </ul>
            </body>
        </html>
    };
    Html(leptos::prelude::RenderHtml::to_html(html)).into_response()
}

/// Server-rendered page shell (no client-side hydration/wasm — Leptos here
/// is just producing the static HTML; all interactivity is FullCalendar's
/// own JS, loaded from a CDN, talking to the JSON API below).
pub async fn handle_webview_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(db_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_owned_calendar(&state, &claims, &db_id).await {
        return status.into_response();
    }

    let events_url = format!("/app/{}/api/events", db_id);
    let html = leptos::prelude::view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <title>"Notion Calendar"</title>
                <link
                    rel="stylesheet"
                    href="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.css"
                />
                <style>
                    "body { font-family: sans-serif; margin: 2rem; }
                    #calendar { max-width: 1000px; margin: 0 auto; }
                    nav { max-width: 1000px; margin: 0 auto 1rem; }"
                </style>
            </head>
            <body>
                <nav><a href="/app">"← All calendars"</a></nav>
                <div id="calendar"></div>
                <script src="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js"></script>
                <script>{webview_js(&events_url)}</script>
            </body>
        </html>
    };
    Html(leptos::prelude::RenderHtml::to_html(html)).into_response()
}

fn webview_js(events_url: &str) -> String {
    format!(
        r#"
document.addEventListener('DOMContentLoaded', function() {{
  var calendarEl = document.getElementById('calendar');
  var calendar = new FullCalendar.Calendar(calendarEl, {{
    initialView: 'dayGridMonth',
    headerToolbar: {{ left: 'prev,next today', center: 'title', right: 'dayGridMonth,timeGridWeek,listWeek' }},
    selectable: true,
    editable: true,
    events: '{events_url}',

    // Click an empty date/slot to create a new event.
    select: function(info) {{
      var title = prompt('Tên sự kiện:');
      if (!title) {{ calendar.unselect(); return; }}
      fetch('{events_url}', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ title: title, start: info.startStr, end: info.endStr }})
      }}).then(function(r) {{
        if (!r.ok) {{ alert('Tạo event thất bại'); return; }}
        calendar.refetchEvents();
      }});
      calendar.unselect();
    }},

    // Click an existing event to rename or delete it.
    eventClick: function(info) {{
      var action = prompt('Sửa tên event (để trống + OK = xoá, Cancel = bỏ qua):', info.event.title);
      if (action === null) return;
      if (action === '') {{
        if (!confirm('Xoá event "' + info.event.title + '"?')) return;
        fetch('{events_url}/' + encodeURIComponent(info.event.id), {{ method: 'DELETE' }})
          .then(function(r) {{
            if (!r.ok) {{ alert('Xoá thất bại'); return; }}
            calendar.refetchEvents();
          }});
        return;
      }}
      fetch('{events_url}/' + encodeURIComponent(info.event.id), {{
        method: 'PATCH',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ title: action }})
      }}).then(function(r) {{
        if (!r.ok) {{ alert('Sửa thất bại'); return; }}
        calendar.refetchEvents();
      }});
    }},

    // Drag/resize an event to move its date.
    eventDrop: function(info) {{ patchEventDates(info); }},
    eventResize: function(info) {{ patchEventDates(info); }},
  }});

  function patchEventDates(info) {{
    fetch('{events_url}/' + encodeURIComponent(info.event.id), {{
      method: 'PATCH',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{
        start: info.event.startStr,
        end: info.event.endStr || null
      }})
    }}).then(function(r) {{
      if (!r.ok) {{ alert('Cập nhật ngày thất bại'); info.revert(); }}
    }});
  }}

  calendar.render();
}});
"#,
        events_url = events_url
    )
}

pub async fn handle_list_events(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(db_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_owned_calendar(&state, &claims, &db_id).await {
        return status.into_response();
    }

    let cache = state.cache.read().await;
    let pages = cache.get(&db_id).cloned().unwrap_or_default();
    let events: Vec<_> = pages
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "title": p.title,
                "start": p.start,
                "end": p.end,
                "url": p.url,
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
}

pub async fn handle_create_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(db_id): Path<String>,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &db_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    match state
        .notion_create_event(
            &cal.data_source_id,
            &cal.date_property,
            &cal.notion_access_token,
            &body.title,
            &body.start,
            body.end.as_deref(),
        )
        .await
    {
        Ok(page_id) => {
            state.refresh_by_data_source(&cal.data_source_id).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "id": page_id }))).into_response()
        }
        Err(e) => {
            error!("webview create event failed: {}", e);
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateEventBody {
    title: Option<String>,
    start: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    end: Option<Option<String>>,
}

// Distinguishes "end omitted" (None) from "end explicitly cleared" (Some(None))
// so an all-day drag doesn't accidentally leave a stale end date behind.
fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

pub async fn handle_update_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path((db_id, event_id)): Path<(String, String)>,
    Json(body): Json<UpdateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &db_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    match state
        .notion_update_event(
            &event_id,
            &cal.date_property,
            &cal.notion_access_token,
            body.title.as_deref(),
            body.start.as_deref(),
            body.end.as_ref().map(|o| o.as_deref()),
        )
        .await
    {
        Ok(()) => {
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview update event failed: {}", e);
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

pub async fn handle_delete_event(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path((db_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &db_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    match state.notion_delete_event(&event_id, &cal.notion_access_token).await {
        Ok(()) => {
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview delete event failed: {}", e);
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}
