use axum::extract::{Path, State};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use tracing::error;

use crate::session::owned_calendar_or_error;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct SyncLogRow {
    occurred_at: String,
    source: String,
    action: String,
    event_uid: String,
    notion_page_id: String,
    status: String,
    detail: String,
}

impl From<SyncLogRow> for app::sync_log::SyncLogRow {
    fn from(r: SyncLogRow) -> Self {
        app::sync_log::SyncLogRow {
            occurred_at: r.occurred_at,
            source: r.source,
            action: r.action,
            event_uid: r.event_uid,
            notion_page_id: r.notion_page_id,
            status: r.status,
            detail: r.detail,
        }
    }
}

struct SyncLogLabels {
    html_lang: &'static str,
    heading_label: &'static str,
    col_time: &'static str,
    col_source: &'static str,
    col_action: &'static str,
    col_event_uid: &'static str,
    col_notion_page: &'static str,
    col_result: &'static str,
    empty_state: &'static str,
    status_ok: &'static str,
    status_error: &'static str,
}

const SYNC_LOG_LABELS_VI: SyncLogLabels = SyncLogLabels {
    html_lang: "vi",
    heading_label: "Log đồng bộ",
    col_time: "Thời gian",
    col_source: "Nguồn",
    col_action: "Hành động",
    col_event_uid: "UID sự kiện",
    col_notion_page: "Notion page",
    col_result: "Kết quả",
    empty_state: "Chưa có hoạt động đồng bộ nào được ghi lại.",
    status_ok: "OK",
    status_error: "Lỗi",
};

const SYNC_LOG_LABELS_EN: SyncLogLabels = SyncLogLabels {
    html_lang: "en",
    heading_label: "Sync log",
    col_time: "Time",
    col_source: "Source",
    col_action: "Action",
    col_event_uid: "Event UID",
    col_notion_page: "Notion page",
    col_result: "Result",
    empty_state: "No sync activity has been recorded yet.",
    status_ok: "OK",
    status_error: "Error",
};

pub async fn sync_log_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let rows: Vec<SyncLogRow> = sqlx::query_as(
        "SELECT occurred_at::text AS occurred_at, source, action, event_uid, notion_page_id, status, detail
         FROM sync_log WHERE calendar_id = $1 ORDER BY occurred_at DESC LIMIT 200",
    )
    .bind(cal.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| {
        error!("failed to load sync_log for calendar {}: {}", cal.id, e);
        Vec::new()
    });

    let l = match lang {
        crate::i18n::Lang::En => &SYNC_LOG_LABELS_EN,
        crate::i18n::Lang::Vi => &SYNC_LOG_LABELS_VI,
    };
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let data = app::sync_log::SyncLogPageData {
        html_lang: l.html_lang.to_string(),
        top_nav_html: crate::i18n::top_nav_html(
            email,
            lang,
            &format!("/me/calendars/{public_id}/log"),
        ),
        calendar_name: cal.display_name,
        heading_label: l.heading_label.to_string(),
        col_time: l.col_time.to_string(),
        col_source: l.col_source.to_string(),
        col_action: l.col_action.to_string(),
        col_event_uid: l.col_event_uid.to_string(),
        col_notion_page: l.col_notion_page.to_string(),
        col_result: l.col_result.to_string(),
        empty_state: l.empty_state.to_string(),
        status_ok: l.status_ok.to_string(),
        status_error: l.status_error.to_string(),
        rows: rows.into_iter().map(Into::into).collect(),
    };

    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::sync_log::SyncLogShell data=data/> }
    });
    handler(request).await
}
