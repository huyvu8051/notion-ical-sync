use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::auth::{find_or_create_user, html_escape};
use crate::AppState;

async fn current_user_id(state: &AppState, claims: &OidcClaims<EmptyAdditionalClaims>) -> Result<i64, StatusCode> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    find_or_create_user(&state.db, sub, email).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Every `/app/{public_id}/...` handler needs this same check: the calendar
/// must exist, and must belong to whoever is logged in — otherwise one user
/// could read or edit another user's Notion events just by guessing an id.
/// Looked up by public_id, not database_id: several users can now each have
/// their own subscription to the same Notion database, so database_id alone
/// no longer identifies a single owner.
async fn require_owned_calendar(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
    public_id: &str,
) -> Result<crate::caldav::CalendarRow, StatusCode> {
    let user_id = current_user_id(state, claims).await?;
    match state.calendar_by_public_id(public_id).await {
        Some(cal) if cal.user_id == user_id => Ok(cal),
        Some(_) => Err(StatusCode::FORBIDDEN),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Server-rendered page shell (no client-side hydration/wasm — Leptos here
/// is just producing the static HTML; FullCalendar (CDN) is still the actual
/// calendar engine — this only restyles the surrounding chrome/nav and
/// replaces the old prompt()/confirm() add/edit/delete flow with a real
/// modal, per the Stitch "Chi tiết lịch" mockup (project 7966553897766226544,
/// screen 7a5ff90c2ebc48b4b202f9061739819e).
pub async fn handle_webview_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    let calendar_name = if cal.display_name.is_empty() { "Notion Calendar".to_string() } else { cal.display_name.clone() };

    let events_url = format!("/app/{}/api/events", public_id);
    Html(format!(
        r##"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — NotionCal</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.css">
<script src="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js"></script>
<script src="https://cdn.tailwindcss.com?plugins=forms"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet">
<script id="tailwind-config">
tailwind.config = {{
  theme: {{ extend: {{
    colors: {{
      "outline-variant": "#e5e5e5", "outline": "#747878", "on-surface": "#1b1c1c",
      "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
      "secondary": "#3B82F6", "error": "#ba1a1a", "surface-container-low": "#f5f3f3",
      "on-surface-variant": "#444748"
    }},
    spacing: {{ "md": "16px", "lg": "24px", "sm": "8px", "xs": "4px" }},
    fontFamily: {{ "sans": ["Inter"], "code": ["Geist"] }},
    fontSize: {{
      "h1": ["24px", {{ lineHeight: "1.3", fontWeight: "600" }}],
      "h2": ["20px", {{ lineHeight: "1.4", fontWeight: "600" }}],
      "label-md": ["13px", {{ lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }}]
    }}
  }} }}
}}
</script>
<style>
body {{ background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }}
.material-symbols-outlined {{ font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }}
.modal-shadow {{ box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }}
#calendar {{ max-width: 1100px; margin: 0 auto; padding: 24px; }}
.fc {{ --fc-border-color: #e5e5e5; --fc-button-bg-color: #fff; --fc-button-border-color: #e5e5e5; --fc-button-text-color: #1b1c1c;
  --fc-button-active-bg-color: #000; --fc-button-active-border-color: #000; --fc-today-bg-color: #f5f3f3; font-family: 'Inter', sans-serif; }}
.fc .fc-button {{ box-shadow: none !important; text-transform: none; font-weight: 500; }}
</style>
</head>
<body class="bg-background text-on-surface">
<header class="h-16 flex items-center justify-between px-lg border-b border-outline-variant bg-surface">
<div class="flex items-center gap-md">
<a class="flex items-center gap-xs text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/me">
<span class="material-symbols-outlined">arrow_back</span>
Tất cả lịch
</a>
<div class="h-6 w-[1px] bg-outline-variant mx-sm"></div>
<h1 class="text-h1 tracking-tight">{title}</h1>
</div>
<button class="bg-primary text-on-primary px-md h-10 flex items-center gap-xs text-label-md rounded-lg hover:opacity-90 transition-opacity" onclick="openCreateModal()">
<span class="material-symbols-outlined">add</span>
Thêm sự kiện
</button>
</header>
<div id="calendar"></div>

<div class="fixed inset-0 bg-black/40 z-50 hidden items-center justify-center" id="modal-backdrop">
<div class="bg-white w-full max-w-lg mx-4 rounded-xl modal-shadow overflow-hidden" id="modal-content">
<div class="px-lg py-md border-b border-outline-variant flex items-center justify-between">
<h2 class="text-h2" id="modal-title">Thêm sự kiện</h2>
<button class="p-1 hover:bg-surface-container-low rounded-lg transition-colors" onclick="closeModal()">
<span class="material-symbols-outlined">close</span>
</button>
</div>
<div class="p-lg space-y-lg">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">Tên sự kiện</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-title" placeholder="Nhập tên sự kiện..." type="text">
</div>
<div class="grid grid-cols-2 gap-md">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">Bắt đầu</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-start" type="datetime-local">
</div>
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">Kết thúc</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-end" type="datetime-local">
</div>
</div>
<div class="flex items-center gap-sm">
<input class="w-4 h-4 rounded text-primary focus:ring-primary border-outline-variant" id="modal-field-allday" type="checkbox">
<label class="cursor-pointer" for="modal-field-allday">Cả ngày</label>
</div>
<a class="hidden items-center gap-xs text-secondary hover:underline text-label-md" href="#" id="modal-notion-link" target="_blank" rel="noopener">
Mở trong Notion
<span class="material-symbols-outlined text-[14px]">open_in_new</span>
</a>
</div>
<div class="px-lg py-md bg-surface-container-low flex items-center justify-between">
<button class="text-error text-label-md hover:underline hidden" id="modal-delete-btn" onclick="deleteFromModal()">Xoá</button>
<div class="flex items-center gap-md ml-auto">
<button class="px-md h-10 border border-outline-variant rounded-lg bg-white hover:bg-surface-container-low text-label-md transition-colors" onclick="closeModal()">Huỷ</button>
<button class="bg-primary text-on-primary px-lg h-10 rounded-lg text-label-md hover:opacity-90 transition-opacity" onclick="saveFromModal()">Lưu</button>
</div>
</div>
</div>
</div>

<script>{js}</script>
</body></html>"##,
        title = html_escape(&calendar_name),
        js = webview_js(&events_url),
    ))
    .into_response()
}

fn webview_js(events_url: &str) -> String {
    format!(
        r#"
var calendar;
var editingEventId = null;

function toLocalInputValue(dateStr) {{
  if (!dateStr) return '';
  var d = new Date(dateStr);
  var pad = function(n) {{ return String(n).padStart(2, '0'); }};
  return d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()) + 'T' + pad(d.getHours()) + ':' + pad(d.getMinutes());
}}

function openCreateModal(startStr, endStr, allDay) {{
  editingEventId = null;
  document.getElementById('modal-title').textContent = 'Thêm sự kiện';
  document.getElementById('modal-field-title').value = '';
  document.getElementById('modal-field-start').value = startStr ? toLocalInputValue(startStr) : '';
  document.getElementById('modal-field-end').value = endStr ? toLocalInputValue(endStr) : '';
  document.getElementById('modal-field-allday').checked = !!allDay;
  document.getElementById('modal-notion-link').classList.add('hidden');
  document.getElementById('modal-delete-btn').classList.add('hidden');
  showModal();
}}

function openEditModal(info) {{
  editingEventId = info.event.id;
  document.getElementById('modal-title').textContent = 'Chỉnh sửa sự kiện';
  document.getElementById('modal-field-title').value = info.event.title;
  document.getElementById('modal-field-start').value = toLocalInputValue(info.event.startStr);
  document.getElementById('modal-field-end').value = info.event.endStr ? toLocalInputValue(info.event.endStr) : '';
  document.getElementById('modal-field-allday').checked = info.event.allDay;
  var notionUrl = info.event.extendedProps.notionUrl;
  var link = document.getElementById('modal-notion-link');
  if (notionUrl) {{
    link.href = notionUrl;
    link.classList.remove('hidden');
    link.classList.add('inline-flex');
  }} else {{
    link.classList.add('hidden');
  }}
  document.getElementById('modal-delete-btn').classList.remove('hidden');
  showModal();
}}

function showModal() {{
  var backdrop = document.getElementById('modal-backdrop');
  backdrop.classList.remove('hidden');
  backdrop.classList.add('flex');
}}

function closeModal() {{
  var backdrop = document.getElementById('modal-backdrop');
  backdrop.classList.add('hidden');
  backdrop.classList.remove('flex');
}}

function saveFromModal() {{
  var title = document.getElementById('modal-field-title').value.trim();
  if (!title) {{ alert('Nhập tên sự kiện'); return; }}
  var allDay = document.getElementById('modal-field-allday').checked;
  var startVal = document.getElementById('modal-field-start').value;
  var endVal = document.getElementById('modal-field-end').value;
  if (!startVal) {{ alert('Chọn ngày bắt đầu'); return; }}
  var start = allDay ? startVal.slice(0, 10) : startVal;
  var end = endVal ? (allDay ? endVal.slice(0, 10) : endVal) : null;

  if (editingEventId) {{
    fetch('{events_url}/' + encodeURIComponent(editingEventId), {{
      method: 'PATCH',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ title: title, start: start, end: end }})
    }}).then(function(r) {{
      if (!r.ok) {{ alert('Cập nhật thất bại'); return; }}
      closeModal();
      calendar.refetchEvents();
    }});
  }} else {{
    fetch('{events_url}', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ title: title, start: start, end: end }})
    }}).then(function(r) {{
      if (!r.ok) {{ alert('Tạo event thất bại'); return; }}
      closeModal();
      calendar.refetchEvents();
    }});
  }}
}}

function deleteFromModal() {{
  if (!editingEventId) return;
  if (!confirm('Xoá sự kiện này?')) return;
  fetch('{events_url}/' + encodeURIComponent(editingEventId), {{ method: 'DELETE' }})
    .then(function(r) {{
      if (!r.ok) {{ alert('Xoá thất bại'); return; }}
      closeModal();
      calendar.refetchEvents();
    }});
}}

document.addEventListener('DOMContentLoaded', function() {{
  var calendarEl = document.getElementById('calendar');
  calendar = new FullCalendar.Calendar(calendarEl, {{
    initialView: 'dayGridMonth',
    headerToolbar: {{ left: 'prev,next today', center: 'title', right: 'dayGridMonth,timeGridWeek,listWeek' }},
    selectable: true,
    editable: true,
    events: '{events_url}',

    // Click/drag an empty date or time slot to open the create modal
    // pre-filled with the selected range.
    select: function(info) {{
      openCreateModal(info.startStr, info.endStr, info.allDay);
      calendar.unselect();
    }},

    // Click an existing event to open the edit modal.
    eventClick: function(info) {{
      openEditModal(info);
    }},

    // Drag/resize an event to move its date — applied immediately, no modal
    // (matches the drag gesture's own implicit confirmation).
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
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
                // Not FullCalendar's special top-level "url" field (that
                // makes clicking navigate away instead of firing eventClick)
                // — this lands in event.extendedProps.notionUrl instead, for
                // the edit modal's "Mở trong Notion" link.
                "notionUrl": p.url,
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
    Path(public_id): Path<String>,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
    Path((public_id, event_id)): Path<(String, String)>,
    Json(body): Json<UpdateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
    Path((public_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
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
