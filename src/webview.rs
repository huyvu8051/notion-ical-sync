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

struct WebviewLabels {
    html_lang: &'static str,
    back_to_all: &'static str,
    add_event_btn: &'static str,
    modal_title_add: &'static str,
    event_name_label: &'static str,
    event_name_placeholder: &'static str,
    start_label: &'static str,
    end_label: &'static str,
    allday_label: &'static str,
    open_in_notion: &'static str,
    delete_btn: &'static str,
    cancel_btn: &'static str,
    save_btn: &'static str,
    modal_title_edit: &'static str,
    alert_enter_title: &'static str,
    alert_pick_start: &'static str,
    alert_update_failed: &'static str,
    alert_create_failed: &'static str,
    confirm_delete_event: &'static str,
    alert_delete_failed: &'static str,
    alert_update_date_failed: &'static str,
    alert_quota_exceeded: &'static str,
}

const WEBVIEW_LABELS_VI: WebviewLabels = WebviewLabels {
    html_lang: "vi",
    back_to_all: "Tất cả lịch",
    add_event_btn: "Thêm sự kiện",
    modal_title_add: "Thêm sự kiện",
    event_name_label: "Tên sự kiện",
    event_name_placeholder: "Nhập tên sự kiện...",
    start_label: "Bắt đầu",
    end_label: "Kết thúc",
    allday_label: "Cả ngày",
    open_in_notion: "Mở trong Notion",
    delete_btn: "Xoá",
    cancel_btn: "Huỷ",
    save_btn: "Lưu",
    modal_title_edit: "Chỉnh sửa sự kiện",
    alert_enter_title: "Nhập tên sự kiện",
    alert_pick_start: "Chọn ngày bắt đầu",
    alert_update_failed: "Cập nhật thất bại",
    alert_create_failed: "Tạo event thất bại",
    confirm_delete_event: "Xoá sự kiện này?",
    alert_delete_failed: "Xoá thất bại",
    alert_update_date_failed: "Cập nhật ngày thất bại",
    alert_quota_exceeded: "Đã đạt giới hạn 10 sự kiện miễn phí hôm nay. Nâng cấp $1/năm để bỏ giới hạn.",
};

const WEBVIEW_LABELS_EN: WebviewLabels = WebviewLabels {
    html_lang: "en",
    back_to_all: "All calendars",
    add_event_btn: "Add event",
    modal_title_add: "Add event",
    event_name_label: "Event name",
    event_name_placeholder: "Enter event name...",
    start_label: "Start",
    end_label: "End",
    allday_label: "All day",
    open_in_notion: "Open in Notion",
    delete_btn: "Delete",
    cancel_btn: "Cancel",
    save_btn: "Save",
    modal_title_edit: "Edit event",
    alert_enter_title: "Enter an event name",
    alert_pick_start: "Pick a start date",
    alert_update_failed: "Update failed",
    alert_create_failed: "Failed to create event",
    confirm_delete_event: "Delete this event?",
    alert_delete_failed: "Delete failed",
    alert_update_date_failed: "Failed to update date",
    alert_quota_exceeded: "You've hit today's free limit of 10 events. Upgrade for $1/year to remove it.",
};

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
    lang: crate::i18n::Lang,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    let l = match lang {
        crate::i18n::Lang::En => &WEBVIEW_LABELS_EN,
        crate::i18n::Lang::Vi => &WEBVIEW_LABELS_VI,
    };
    let calendar_name = if cal.display_name.is_empty() { "Notion Calendar".to_string() } else { cal.display_name.clone() };
    let lang_toggle = crate::i18n::lang_toggle(lang, &format!("/app/{}", public_id));

    let events_url = format!("/app/{}/api/events", public_id);
    Html(format!(
        r##"<!doctype html>
<html lang="{html_lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
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
{back_to_all}
</a>
<div class="h-6 w-[1px] bg-outline-variant mx-sm"></div>
<h1 class="text-h1 tracking-tight">{title}</h1>
</div>
<div class="flex items-center gap-md">
{lang_toggle}
<button class="bg-primary text-on-primary px-md h-10 flex items-center gap-xs text-label-md rounded-lg hover:opacity-90 transition-opacity" onclick="openCreateModal()">
<span class="material-symbols-outlined">add</span>
{add_event_btn}
</button>
</div>
</header>
<div id="calendar"></div>

<div class="fixed inset-0 bg-black/40 z-50 hidden items-center justify-center" id="modal-backdrop">
<div class="bg-white w-full max-w-lg mx-4 rounded-xl modal-shadow overflow-hidden" id="modal-content">
<div class="px-lg py-md border-b border-outline-variant flex items-center justify-between">
<h2 class="text-h2" id="modal-title">{modal_title_add}</h2>
<button class="p-1 hover:bg-surface-container-low rounded-lg transition-colors" onclick="closeModal()">
<span class="material-symbols-outlined">close</span>
</button>
</div>
<div class="p-lg space-y-lg">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{event_name_label}</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-title" placeholder="{event_name_placeholder}" type="text">
</div>
<div class="grid grid-cols-2 gap-md">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{start_label}</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-start" type="datetime-local">
</div>
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{end_label}</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-end" type="datetime-local">
</div>
</div>
<div class="flex items-center gap-sm">
<input class="w-4 h-4 rounded text-primary focus:ring-primary border-outline-variant" id="modal-field-allday" type="checkbox">
<label class="cursor-pointer" for="modal-field-allday">{allday_label}</label>
</div>
<a class="hidden items-center gap-xs text-secondary hover:underline text-label-md" href="#" id="modal-notion-link" target="_blank" rel="noopener">
{open_in_notion}
<span class="material-symbols-outlined text-[14px]">open_in_new</span>
</a>
</div>
<div class="px-lg py-md bg-surface-container-low flex items-center justify-between">
<button class="text-error text-label-md hover:underline hidden" id="modal-delete-btn" onclick="deleteFromModal()">{delete_btn}</button>
<div class="flex items-center gap-md ml-auto">
<button class="px-md h-10 border border-outline-variant rounded-lg bg-white hover:bg-surface-container-low text-label-md transition-colors" onclick="closeModal()">{cancel_btn}</button>
<button class="bg-primary text-on-primary px-lg h-10 rounded-lg text-label-md hover:opacity-90 transition-opacity" onclick="saveFromModal()">{save_btn}</button>
</div>
</div>
</div>
</div>

<script>{js}</script>
</body></html>"##,
        html_lang = l.html_lang,
        title = html_escape(&calendar_name),
        lang_toggle = lang_toggle,
        back_to_all = l.back_to_all,
        add_event_btn = l.add_event_btn,
        modal_title_add = l.modal_title_add,
        event_name_label = l.event_name_label,
        event_name_placeholder = l.event_name_placeholder,
        start_label = l.start_label,
        end_label = l.end_label,
        allday_label = l.allday_label,
        open_in_notion = l.open_in_notion,
        delete_btn = l.delete_btn,
        cancel_btn = l.cancel_btn,
        save_btn = l.save_btn,
        js = webview_js(&events_url, l),
    ))
    .into_response()
}

fn webview_js(events_url: &str, l: &WebviewLabels) -> String {
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
  document.getElementById('modal-title').textContent = '{modal_title_add}';
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
  document.getElementById('modal-title').textContent = '{modal_title_edit}';
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
  if (!title) {{ alert('{alert_enter_title}'); return; }}
  var allDay = document.getElementById('modal-field-allday').checked;
  var startVal = document.getElementById('modal-field-start').value;
  var endVal = document.getElementById('modal-field-end').value;
  if (!startVal) {{ alert('{alert_pick_start}'); return; }}
  var start = allDay ? startVal.slice(0, 10) : startVal;
  var end = endVal ? (allDay ? endVal.slice(0, 10) : endVal) : null;

  if (editingEventId) {{
    fetch('{events_url}/' + encodeURIComponent(editingEventId), {{
      method: 'PATCH',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ title: title, start: start, end: end }})
    }}).then(function(r) {{
      if (!r.ok) {{ alert(r.status === 429 ? '{alert_quota_exceeded}' : '{alert_update_failed}'); return; }}
      closeModal();
      calendar.refetchEvents();
    }});
  }} else {{
    fetch('{events_url}', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ title: title, start: start, end: end }})
    }}).then(function(r) {{
      if (!r.ok) {{ alert(r.status === 429 ? '{alert_quota_exceeded}' : '{alert_create_failed}'); return; }}
      closeModal();
      calendar.refetchEvents();
    }});
  }}
}}

function deleteFromModal() {{
  if (!editingEventId) return;
  if (!confirm('{confirm_delete_event}')) return;
  fetch('{events_url}/' + encodeURIComponent(editingEventId), {{ method: 'DELETE' }})
    .then(function(r) {{
      if (!r.ok) {{ alert('{alert_delete_failed}'); return; }}
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
      if (!r.ok) {{ alert('{alert_update_date_failed}'); info.revert(); }}
    }});
  }}

  calendar.render();
}});
"#,
        events_url = events_url,
        modal_title_add = l.modal_title_add,
        modal_title_edit = l.modal_title_edit,
        alert_enter_title = l.alert_enter_title,
        alert_pick_start = l.alert_pick_start,
        alert_update_failed = l.alert_update_failed,
        alert_create_failed = l.alert_create_failed,
        confirm_delete_event = l.confirm_delete_event,
        alert_delete_failed = l.alert_delete_failed,
        alert_update_date_failed = l.alert_update_date_failed,
        alert_quota_exceeded = l.alert_quota_exceeded,
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
    lang: crate::i18n::Lang,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    if crate::billing::enforce_quota(&state, cal.user_id).await.is_err() {
        state.log_sync(cal.id, "webview", "create", "", "", "error", "daily quota exceeded").await;
        let l = match lang {
            crate::i18n::Lang::En => &WEBVIEW_LABELS_EN,
            crate::i18n::Lang::Vi => &WEBVIEW_LABELS_VI,
        };
        return (StatusCode::TOO_MANY_REQUESTS, l.alert_quota_exceeded).into_response();
    }
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
            state.log_sync(cal.id, "webview", "create", "", &page_id, "ok", "").await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "id": page_id }))).into_response()
        }
        Err(e) => {
            error!("webview create event failed: {}", e);
            state.log_sync(cal.id, "webview", "create", "", "", "error", &e).await;
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
    lang: crate::i18n::Lang,
    Json(body): Json<UpdateEventBody>,
) -> impl IntoResponse {
    let cal = match require_owned_calendar(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(status) => return status.into_response(),
    };
    if crate::billing::enforce_quota(&state, cal.user_id).await.is_err() {
        state.log_sync(cal.id, "webview", "update", &event_id, "", "error", "daily quota exceeded").await;
        let l = match lang {
            crate::i18n::Lang::En => &WEBVIEW_LABELS_EN,
            crate::i18n::Lang::Vi => &WEBVIEW_LABELS_VI,
        };
        return (StatusCode::TOO_MANY_REQUESTS, l.alert_quota_exceeded).into_response();
    }
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
            state.log_sync(cal.id, "webview", "update", &event_id, &event_id, "ok", "").await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview update event failed: {}", e);
            state.log_sync(cal.id, "webview", "update", &event_id, &event_id, "error", &e).await;
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
            state.log_sync(cal.id, "webview", "delete", &event_id, &event_id, "ok", "").await;
            state.refresh_by_data_source(&cal.data_source_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("webview delete event failed: {}", e);
            state.log_sync(cal.id, "webview", "delete", &event_id, &event_id, "error", &e).await;
            (StatusCode::BAD_GATEWAY, e).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the exact class of bug that shipped a blank
    /// webview page to production (2026-08-02): embedded JS inside a Rust
    /// `format!` string can be syntactically invalid JavaScript while still
    /// compiling fine as Rust, since `cargo build` never parses the string
    /// contents. Shells out to `node --check` on the actual rendered output
    /// for both languages; skips (doesn't fail) if node isn't installed,
    /// since it's not required for `cargo build`/CI, just a local dev aid.
    #[test]
    fn webview_js_is_valid_javascript_in_both_languages() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping: node not installed");
            return;
        }

        for labels in [&WEBVIEW_LABELS_VI, &WEBVIEW_LABELS_EN] {
            let js = webview_js("/app/test-public-id/api/events", labels);
            let mut child = Command::new("node")
                .arg("--check")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to spawn node");
            child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "webview_js produced invalid JavaScript for lang={}: {}",
                labels.html_lang,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
