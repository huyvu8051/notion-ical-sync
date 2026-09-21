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

    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let data = app::sync_log::SyncLogPageData {
        top_nav_html: crate::i18n::top_nav_html(
            email,
            lang,
            &format!("/me/calendars/{public_id}/log"),
        ),
        calendar_name: cal.display_name,
        rows: rows.into_iter().map(Into::into).collect(),
    };

    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::sync_log::SyncLogShell data=data/> }
    });
    handler(request).await
}
