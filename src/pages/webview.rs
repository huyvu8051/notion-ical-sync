use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};

use crate::error_page::{error_page, OauthError};
use crate::session::{html_escape, require_owned_calendar};
use crate::AppState;

fn webview_labels_vi() -> app::webview::WebviewLabels {
    app::webview::WebviewLabels {
        modal_title_add: "Thêm sự kiện".to_string(),
        modal_title_edit: "Chỉnh sửa sự kiện".to_string(),
        event_name_label: "Tên sự kiện".to_string(),
        event_name_placeholder: "Nhập tên sự kiện...".to_string(),
        start_label: "Bắt đầu".to_string(),
        end_label: "Kết thúc".to_string(),
        allday_label: "Cả ngày".to_string(),
        location_label: "Địa điểm".to_string(),
        location_placeholder: "Nhập địa điểm...".to_string(),
        location_no_results: "Không tìm thấy kết quả".to_string(),
        notes_label: "Ghi chú".to_string(),
        notes_placeholder: "Nhập ghi chú...".to_string(),
        priority_label: "Mức độ ưu tiên".to_string(),
        priority_none: "Không đặt".to_string(),
        priority_high: "Cao".to_string(),
        priority_medium: "Trung bình".to_string(),
        priority_low: "Thấp".to_string(),
        busy_label: "Trạng thái".to_string(),
        busy_unset: "Không đặt".to_string(),
        busy_busy: "Bận".to_string(),
        busy_free: "Rảnh".to_string(),
        reminder_label: "Nhắc nhở".to_string(),
        reminder_none: "Không nhắc".to_string(),
        reminder_5min: "5 phút trước".to_string(),
        reminder_15min: "15 phút trước".to_string(),
        reminder_30min: "30 phút trước".to_string(),
        reminder_1hour: "1 giờ trước".to_string(),
        reminder_1day: "1 ngày trước".to_string(),
        reminder_2days: "2 ngày trước".to_string(),
        reminder_1week: "1 tuần trước".to_string(),
        reminder_custom: "Tuỳ chỉnh...".to_string(),
        reminder_custom_minutes: "Phút".to_string(),
        reminder_custom_hours: "Giờ".to_string(),
        reminder_custom_days: "Ngày".to_string(),
        travel_time_label: "Thời gian di chuyển".to_string(),
        travel_none: "Không đặt".to_string(),
        travel_0min: "0 phút".to_string(),
        travel_15min: "15 phút".to_string(),
        travel_30min: "30 phút".to_string(),
        travel_45min: "45 phút".to_string(),
        travel_1hour: "1 giờ".to_string(),
        travel_90min: "1.5 giờ".to_string(),
        travel_custom: "Tuỳ chỉnh...".to_string(),
        open_in_notion: "Mở trong Notion".to_string(),
        delete_btn: "Xoá".to_string(),
        cancel_btn: "Huỷ".to_string(),
        save_btn: "Lưu".to_string(),
        saving_label: "Đang lưu...".to_string(),
        confirm_delete_event: "Xoá sự kiện này?".to_string(),
        repeat_display_prefix: "Lặp lại: ".to_string(),
        attendees_display_prefix: "Người được mời: ".to_string(),
        alert_enter_title: "Nhập tên sự kiện".to_string(),
        alert_pick_start: "Chọn ngày bắt đầu".to_string(),
        alert_update_failed: "Cập nhật thất bại".to_string(),
        alert_create_failed: "Tạo event thất bại".to_string(),
        alert_delete_failed: "Xoá thất bại".to_string(),
        alert_quota_exceeded: "Đã đạt giới hạn 10 sự kiện miễn phí hôm nay. Nâng cấp $1/năm để bỏ giới hạn."
            .to_string(),
    }
}

fn webview_labels_en() -> app::webview::WebviewLabels {
    app::webview::WebviewLabels {
        modal_title_add: "Add event".to_string(),
        modal_title_edit: "Edit event".to_string(),
        event_name_label: "Event name".to_string(),
        event_name_placeholder: "Enter event name...".to_string(),
        start_label: "Start".to_string(),
        end_label: "End".to_string(),
        allday_label: "All day".to_string(),
        location_label: "Location".to_string(),
        location_placeholder: "Enter location...".to_string(),
        location_no_results: "No results found".to_string(),
        notes_label: "Notes".to_string(),
        notes_placeholder: "Enter notes...".to_string(),
        priority_label: "Priority".to_string(),
        priority_none: "Not set".to_string(),
        priority_high: "High".to_string(),
        priority_medium: "Medium".to_string(),
        priority_low: "Low".to_string(),
        busy_label: "Status".to_string(),
        busy_unset: "Not set".to_string(),
        busy_busy: "Busy".to_string(),
        busy_free: "Free".to_string(),
        reminder_label: "Reminder".to_string(),
        reminder_none: "No reminder".to_string(),
        reminder_5min: "5 minutes before".to_string(),
        reminder_15min: "15 minutes before".to_string(),
        reminder_30min: "30 minutes before".to_string(),
        reminder_1hour: "1 hour before".to_string(),
        reminder_1day: "1 day before".to_string(),
        reminder_2days: "2 days before".to_string(),
        reminder_1week: "1 week before".to_string(),
        reminder_custom: "Custom...".to_string(),
        reminder_custom_minutes: "Minutes".to_string(),
        reminder_custom_hours: "Hours".to_string(),
        reminder_custom_days: "Days".to_string(),
        travel_time_label: "Travel time".to_string(),
        travel_none: "Not set".to_string(),
        travel_0min: "0 minutes".to_string(),
        travel_15min: "15 minutes".to_string(),
        travel_30min: "30 minutes".to_string(),
        travel_45min: "45 minutes".to_string(),
        travel_1hour: "1 hour".to_string(),
        travel_90min: "1.5 hours".to_string(),
        travel_custom: "Custom...".to_string(),
        open_in_notion: "Open in Notion".to_string(),
        delete_btn: "Delete".to_string(),
        cancel_btn: "Cancel".to_string(),
        save_btn: "Save".to_string(),
        saving_label: "Saving...".to_string(),
        confirm_delete_event: "Delete this event?".to_string(),
        repeat_display_prefix: "Repeats: ".to_string(),
        attendees_display_prefix: "Invited: ".to_string(),
        alert_enter_title: "Enter an event name".to_string(),
        alert_pick_start: "Pick a start date".to_string(),
        alert_update_failed: "Update failed".to_string(),
        alert_create_failed: "Failed to create event".to_string(),
        alert_delete_failed: "Delete failed".to_string(),
        alert_quota_exceeded: "You've hit today's free limit of 10 events. Upgrade for $1/year to remove it."
            .to_string(),
    }
}

pub(crate) fn labels_for(lang: crate::i18n::Lang) -> app::webview::WebviewLabels {
    match lang {
        crate::i18n::Lang::En => webview_labels_en(),
        crate::i18n::Lang::Vi => webview_labels_vi(),
    }
}

fn back_to_all_label(lang: crate::i18n::Lang) -> &'static str {
    match lang {
        crate::i18n::Lang::Vi => "Tất cả lịch",
        crate::i18n::Lang::En => "All calendars",
    }
}

fn add_event_btn_label(lang: crate::i18n::Lang) -> &'static str {
    match lang {
        crate::i18n::Lang::Vi => "Thêm sự kiện",
        crate::i18n::Lang::En => "Add event",
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
    let calendar_name = if cal.display_name.is_empty() {
        "Notion Calendar".to_string()
    } else {
        cal.display_name.clone()
    };
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let top_nav = crate::i18n::top_nav_html(email, lang, &format!("/app/{}", public_id));

    let events_url = format!("/app/{}/api/events", public_id);
    let escaped_title = html_escape(&calendar_name);

    let header_html = format!(
        r##"{top_nav}
<header class="h-16 flex items-center justify-between px-lg border-b border-outline-variant bg-surface">
<div class="flex items-center gap-md">
<a class="flex items-center gap-xs text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/me">
<span class="material-symbols-outlined">arrow_back</span>
<span class="back-label">{back_to_all}</span>
</a>
<div class="h-6 w-[1px] bg-outline-variant mx-sm"></div>
<h1 class="text-h1 tracking-tight">{escaped_title}</h1>
</div>
<div class="flex items-center gap-md">
<button class="bg-primary text-on-primary px-md h-10 flex items-center gap-xs text-label-md rounded-lg hover:opacity-90 transition-opacity" onclick="window.webview_open_create_modal('', '', false)">
<span class="material-symbols-outlined">add</span>
{add_event_btn}
</button>
</div>
</header>"##,
        back_to_all = back_to_all_label(lang),
        add_event_btn = add_event_btn_label(lang),
    );

    let data = app::webview::WebviewPageData {
        html_lang: lang.code().to_string(),
        title: escaped_title,
        header_html,
        events_url: events_url.clone(),
        mapbox_token: state.mapbox_token.clone(),
        labels: labels_for(lang),
        js_config: app::webview::WebviewJsConfig {
            events_url,
            alert_update_date_failed: match lang {
                crate::i18n::Lang::Vi => "Cập nhật ngày thất bại".to_string(),
                crate::i18n::Lang::En => "Failed to update date".to_string(),
            },
            locale: lang.code().to_string(),
        },
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
