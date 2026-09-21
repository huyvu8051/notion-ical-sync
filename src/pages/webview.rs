use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};

use crate::error_page::{error_page, OauthError};
use crate::session::{html_escape, require_owned_calendar};
use crate::AppState;

pub(crate) struct WebviewLabels {
    pub(crate) html_lang: &'static str,
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
    pub(crate) alert_quota_exceeded: &'static str,
    location_label: &'static str,
    location_placeholder: &'static str,
    notes_label: &'static str,
    notes_placeholder: &'static str,
    priority_label: &'static str,
    priority_none: &'static str,
    priority_high: &'static str,
    priority_medium: &'static str,
    priority_low: &'static str,
    busy_label: &'static str,
    busy_unset: &'static str,
    busy_busy: &'static str,
    busy_free: &'static str,
    reminder_label: &'static str,
    reminder_none: &'static str,
    reminder_5min: &'static str,
    reminder_15min: &'static str,
    reminder_30min: &'static str,
    reminder_1hour: &'static str,
    travel_time_label: &'static str,
    repeat_display_prefix: &'static str,
    attendees_display_prefix: &'static str,
}

pub(crate) const WEBVIEW_LABELS_VI: WebviewLabels = WebviewLabels {
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
    alert_quota_exceeded:
        "Đã đạt giới hạn 10 sự kiện miễn phí hôm nay. Nâng cấp $1/năm để bỏ giới hạn.",
    location_label: "Địa điểm",
    location_placeholder: "Nhập địa điểm...",
    notes_label: "Ghi chú",
    notes_placeholder: "Nhập ghi chú...",
    priority_label: "Mức độ ưu tiên",
    priority_none: "Không đặt",
    priority_high: "Cao",
    priority_medium: "Trung bình",
    priority_low: "Thấp",
    busy_label: "Trạng thái",
    busy_unset: "Không đặt",
    busy_busy: "Bận",
    busy_free: "Rảnh",
    reminder_label: "Nhắc nhở",
    reminder_none: "Không nhắc",
    reminder_5min: "5 phút trước",
    reminder_15min: "15 phút trước",
    reminder_30min: "30 phút trước",
    reminder_1hour: "1 giờ trước",
    travel_time_label: "Thời gian di chuyển (phút)",
    repeat_display_prefix: "Lặp lại: ",
    attendees_display_prefix: "Người được mời: ",
};

pub(crate) const WEBVIEW_LABELS_EN: WebviewLabels = WebviewLabels {
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
    alert_quota_exceeded:
        "You've hit today's free limit of 10 events. Upgrade for $1/year to remove it.",
    location_label: "Location",
    location_placeholder: "Enter location...",
    notes_label: "Notes",
    notes_placeholder: "Enter notes...",
    priority_label: "Priority",
    priority_none: "Not set",
    priority_high: "High",
    priority_medium: "Medium",
    priority_low: "Low",
    busy_label: "Status",
    busy_unset: "Not set",
    busy_busy: "Busy",
    busy_free: "Free",
    reminder_label: "Reminder",
    reminder_none: "No reminder",
    reminder_5min: "5 minutes before",
    reminder_15min: "15 minutes before",
    reminder_30min: "30 minutes before",
    reminder_1hour: "1 hour before",
    travel_time_label: "Travel time (minutes)",
    repeat_display_prefix: "Repeats: ",
    attendees_display_prefix: "Invited: ",
};

pub(crate) fn labels_for(lang: crate::i18n::Lang) -> &'static WebviewLabels {
    match lang {
        crate::i18n::Lang::En => &WEBVIEW_LABELS_EN,
        crate::i18n::Lang::Vi => &WEBVIEW_LABELS_VI,
    }
}

pub async fn handle_webview_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
    lang: crate::i18n::Lang,
    request: axum::extract::Request,
) -> axum::response::Response {
    let cal = match require_owned_calendar(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(StatusCode::FORBIDDEN) => return error_page(lang, OauthError::AccessDenied),
        Err(StatusCode::NOT_FOUND) => return error_page(lang, OauthError::CalendarNotFound),
        Err(status) => return status.into_response(),
    };
    let l = labels_for(lang);
    let calendar_name = if cal.display_name.is_empty() {
        "Notion Calendar".to_string()
    } else {
        cal.display_name.clone()
    };
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let top_nav = crate::i18n::top_nav_html(email, lang, &format!("/app/{}", public_id));

    let events_url = format!("/app/{}/api/events", public_id);

    let body_html = format!(
        r##"{top_nav}
<header class="h-16 flex items-center justify-between px-lg border-b border-outline-variant bg-surface">
<div class="flex items-center gap-md">
<a class="flex items-center gap-xs text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/me">
<span class="material-symbols-outlined">arrow_back</span>
<span class="back-label">{back_to_all}</span>
</a>
<div class="h-6 w-[1px] bg-outline-variant mx-sm"></div>
<h1 class="text-h1 tracking-tight">{title}</h1>
</div>
<div class="flex items-center gap-md">
<button class="bg-primary text-on-primary px-md h-10 flex items-center gap-xs text-label-md rounded-lg hover:opacity-90 transition-opacity" onclick="openCreateModal()">
<span class="material-symbols-outlined">add</span>
{add_event_btn}
</button>
</div>
</header>
<div id="calendar"></div>

<div class="fixed inset-0 bg-black/40 z-50 hidden items-center justify-center" id="modal-backdrop">
<div class="bg-white w-full max-w-lg mx-4 rounded-xl modal-shadow overflow-hidden max-h-[90vh] overflow-y-auto" id="modal-content">
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
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{location_label}</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-location" placeholder="{location_placeholder}" type="text">
</div>
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{notes_label}</label>
<textarea class="w-full px-md py-sm border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-notes" placeholder="{notes_placeholder}" rows="2"></textarea>
</div>
<div class="grid grid-cols-2 gap-md">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{priority_label}</label>
<select class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-priority">
<option value="">{priority_none}</option>
<option value="1">{priority_high}</option>
<option value="5">{priority_medium}</option>
<option value="9">{priority_low}</option>
</select>
</div>
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{busy_label}</label>
<select class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-busy">
<option value="">{busy_unset}</option>
<option value="true">{busy_busy}</option>
<option value="false">{busy_free}</option>
</select>
</div>
</div>
<div class="grid grid-cols-2 gap-md">
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{reminder_label}</label>
<select class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-reminder">
<option value="">{reminder_none}</option>
<option value="5">{reminder_5min}</option>
<option value="15">{reminder_15min}</option>
<option value="30">{reminder_30min}</option>
<option value="60">{reminder_1hour}</option>
</select>
</div>
<div class="space-y-xs">
<label class="text-label-md text-on-surface-variant">{travel_time_label}</label>
<input class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all" id="modal-field-travel" type="number" min="0">
</div>
</div>
<div class="hidden text-label-md text-on-surface-variant" id="modal-repeat-display"></div>
<div class="hidden text-label-md text-on-surface-variant" id="modal-attendees-display"></div>
<a class="hidden items-center gap-xs text-secondary hover:underline text-label-md" href="#" id="modal-notion-link" target="_blank" rel="noopener">
{open_in_notion}
<span class="material-symbols-outlined text-[14px]">open_in_new</span>
</a>
</div>
<div class="px-lg py-md bg-surface-container-low flex items-center justify-between">
<button type="button" id="modal-delete-btn" class="text-error text-label-md hover:underline hidden" onclick="handleDeleteClick()">{delete_btn}</button>
<div class="flex items-center gap-md ml-auto">
<button class="px-md h-10 border border-outline-variant rounded-lg bg-white hover:bg-surface-container-low text-label-md transition-colors" onclick="closeModal()">{cancel_btn}</button>
<button class="bg-primary text-on-primary px-lg h-10 rounded-lg text-label-md hover:opacity-90 transition-opacity" onclick="saveFromModal()">{save_btn}</button>
</div>
</div>
</div>
</div>"##,
        top_nav = top_nav,
        back_to_all = l.back_to_all,
        title = html_escape(&calendar_name),
        add_event_btn = l.add_event_btn,
        modal_title_add = l.modal_title_add,
        event_name_label = l.event_name_label,
        event_name_placeholder = l.event_name_placeholder,
        start_label = l.start_label,
        end_label = l.end_label,
        allday_label = l.allday_label,
        location_label = l.location_label,
        location_placeholder = l.location_placeholder,
        notes_label = l.notes_label,
        notes_placeholder = l.notes_placeholder,
        priority_label = l.priority_label,
        priority_none = l.priority_none,
        priority_high = l.priority_high,
        priority_medium = l.priority_medium,
        priority_low = l.priority_low,
        busy_label = l.busy_label,
        busy_unset = l.busy_unset,
        busy_busy = l.busy_busy,
        busy_free = l.busy_free,
        reminder_label = l.reminder_label,
        reminder_none = l.reminder_none,
        reminder_5min = l.reminder_5min,
        reminder_15min = l.reminder_15min,
        reminder_30min = l.reminder_30min,
        reminder_1hour = l.reminder_1hour,
        travel_time_label = l.travel_time_label,
        open_in_notion = l.open_in_notion,
        delete_btn = l.delete_btn,
        cancel_btn = l.cancel_btn,
        save_btn = l.save_btn,
    );

    let js_config = app::webview::WebviewJsConfig {
        events_url,
        modal_title_add: l.modal_title_add.to_string(),
        modal_title_edit: l.modal_title_edit.to_string(),
        alert_enter_title: l.alert_enter_title.to_string(),
        alert_pick_start: l.alert_pick_start.to_string(),
        alert_update_failed: l.alert_update_failed.to_string(),
        alert_create_failed: l.alert_create_failed.to_string(),
        alert_delete_failed: l.alert_delete_failed.to_string(),
        alert_update_date_failed: l.alert_update_date_failed.to_string(),
        alert_quota_exceeded: l.alert_quota_exceeded.to_string(),
        repeat_display_prefix: l.repeat_display_prefix.to_string(),
        attendees_display_prefix: l.attendees_display_prefix.to_string(),
        delete_btn: l.delete_btn.to_string(),
        confirm_delete_event: l.confirm_delete_event.to_string(),
    };

    let data = app::webview::WebviewPageData {
        html_lang: l.html_lang.to_string(),
        title: html_escape(&calendar_name),
        body_html,
        js_config,
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::webview::WebviewShell data=data/> }
    });
    handler(request).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn static_webview_js_is_valid_javascript() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping: node not installed");
            return;
        }

        let js = include_str!("../../static/webview.js");
        let mut child = Command::new("node")
            .arg("--check")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn node");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(js.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "static/webview.js has invalid JavaScript syntax: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
