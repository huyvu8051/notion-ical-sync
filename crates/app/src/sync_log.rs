use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SyncLogRow {
    pub occurred_at: String,
    pub source: String,
    pub action: String,
    pub event_uid: String,
    pub notion_page_id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SyncLogPageData {
    pub html_lang: String,
    pub top_nav_html: String,
    pub calendar_name: String,
    pub heading_label: String,
    pub col_time: String,
    pub col_source: String,
    pub col_action: String,
    pub col_event_uid: String,
    pub col_notion_page: String,
    pub col_result: String,
    pub empty_state: String,
    pub status_ok: String,
    pub status_error: String,
    pub rows: Vec<SyncLogRow>,
}

#[cfg(feature = "ssr")]
struct SyncLogLabels {
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

#[cfg(feature = "ssr")]
const SYNC_LOG_LABELS: SyncLogLabels = SyncLogLabels {
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

#[cfg(feature = "ssr")]
async fn owned_calendar_id_and_name(
    pool: &sqlx::PgPool,
    claims: &axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims>,
    public_id: &str,
) -> Result<(i64, String), ServerFnError> {
    let sub = claims.subject().as_str();
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, display_name FROM calendars WHERE public_id = $1 AND user_id = \
         (SELECT id FROM users WHERE keycloak_sub = $2)",
    )
    .bind(public_id)
    .bind(sub)
    .fetch_optional(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to look up calendar: {e}")))?;
    row.ok_or_else(|| ServerFnError::new("calendar not found"))
}

#[server]
async fn load_sync_log_data(public_id: String) -> Result<SyncLogPageData, ServerFnError> {
    let claims: axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims> =
        leptos_axum::extract().await?;
    let pool = use_context::<sqlx::PgPool>()
        .ok_or_else(|| ServerFnError::new("missing db pool context"))?;

    let (cal_id, calendar_name) = owned_calendar_id_and_name(&pool, &claims, &public_id).await?;

    let rows: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
         source, action, event_uid, notion_page_id, status, detail
         FROM sync_log WHERE calendar_id = $1 ORDER BY occurred_at DESC LIMIT 200",
    )
    .bind(cal_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to load sync log: {e}")))?;

    let l = &SYNC_LOG_LABELS;
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let top_nav_html = crate::page_shell::top_nav_html(email);

    Ok(SyncLogPageData {
        html_lang: "en".to_string(),
        top_nav_html,
        calendar_name,
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
        rows: rows
            .into_iter()
            .map(
                |(occurred_at, source, action, event_uid, notion_page_id, status, detail)| SyncLogRow {
                    occurred_at,
                    source,
                    action,
                    event_uid,
                    notion_page_id,
                    status,
                    detail,
                },
            )
            .collect(),
    })
}

#[component]
pub fn SyncLogRoutePage() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let public_id = move || params.with(|p| p.get("public_id").unwrap_or_default());
    let data = Resource::new(public_id, load_sync_log_data);
    view! {
        <leptos_meta::Style>{crate::page_shell::ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <Suspense fallback=|| ()>
            {move || data.get().map(|result| match result {
                Ok(data) => view! {
                    <leptos_meta::Html attr:lang=data.html_lang.clone()/>
                    <leptos_meta::Title text=format!("{} — {} — NotionCal", data.heading_label.clone(), data.calendar_name.clone())/>
                    <SyncLogPage data=data/>
                }.into_any(),
                Err(_) => view! {
                    <div class="flex flex-col items-center justify-center py-3xl gap-md text-center">
                        <p class="text-on-surface-variant">"This log wasn't found."</p>
                        <leptos_router::components::A href="/me" attr:class="text-secondary underline">"Back"</leptos_router::components::A>
                    </div>
                }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
pub fn SyncLogPage(data: SyncLogPageData) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        crate::page_shell::install_client_timezone_label();
        crate::page_shell::install_client_local_time_labels();
    }

    let top_nav_html = data.top_nav_html.clone();
    let status_ok = data.status_ok.clone();
    let status_error = data.status_error.clone();
    let heading = format!("{} — {}", data.heading_label, data.calendar_name);

    let body_rows = if data.rows.is_empty() {
        view! {
            <tr>
                <td colspan="6" class="text-center text-on-surface-variant text-body-md py-xl">
                    {data.empty_state.clone()}
                </td>
            </tr>
        }
        .into_any()
    } else {
        data.rows
            .into_iter()
            .map(|row| {
                view! {
                    <SyncLogRowView row=row status_ok=status_ok.clone() status_error=status_error.clone()/>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <div id="sync-log-root">
            <div inner_html=top_nav_html></div>
            <main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
                <div class="flex items-center gap-md">
                    <leptos_router::components::A href="/me" attr:class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded">
                        <span class="material-symbols-outlined">arrow_back</span>
                    </leptos_router::components::A>
                    <h1 class="text-h1 font-semibold">{heading}</h1>
                </div>
                <div class="bg-surface border border-outline-variant rounded-lg overflow-hidden overflow-x-auto">
                    <table class="w-full text-body-md">
                        <thead>
                            <tr class="bg-surface-container-low text-on-surface-variant text-label-md uppercase tracking-wide">
                                <th class="text-left font-medium px-md py-sm">{data.col_time}</th>
                                <th class="text-left font-medium px-md py-sm">{data.col_source}</th>
                                <th class="text-left font-medium px-md py-sm">{data.col_action}</th>
                                <th class="text-left font-medium px-md py-sm">{data.col_event_uid}</th>
                                <th class="text-left font-medium px-md py-sm">{data.col_notion_page}</th>
                                <th class="text-left font-medium px-md py-sm">{data.col_result}</th>
                            </tr>
                        </thead>
                        <tbody>{body_rows}</tbody>
                    </table>
                </div>
                <div inner_html=crate::page_shell::footer_html()></div>
            </main>
        </div>
    }
}

#[component]
fn SyncLogRowView(row: SyncLogRow, status_ok: String, status_error: String) -> impl IntoView {
    let status_class = if row.status == "ok" {
        "text-[#166534] font-semibold"
    } else {
        "text-error font-semibold"
    };
    let status_label = if row.status == "ok" { status_ok } else { status_error };
    let uid_display = if row.event_uid.is_empty() {
        "—".to_string()
    } else {
        row.event_uid.clone()
    };
    let page_link = if row.notion_page_id.is_empty() {
        None
    } else {
        let href = format!("https://notion.so/{}", row.notion_page_id.replace('-', ""));
        let short: String = row.notion_page_id.chars().take(8).collect();
        Some((href, short))
    };
    let detail_view = (!row.detail.is_empty()).then(|| {
        view! {
            <div class="text-error text-label-md whitespace-pre-wrap break-words max-w-xs mt-1">{row.detail.clone()}</div>
        }
    });

    view! {
        <tr class="border-t border-outline-variant align-top">
            <td class="occurred-at px-md py-sm" data-utc=row.occurred_at.clone()>{row.occurred_at.clone()}</td>
            <td class="px-md py-sm">
                <span class="inline-block px-sm py-[2px] rounded bg-surface-container-low text-label-md">{row.source.clone()}</span>
            </td>
            <td class="px-md py-sm">{row.action.clone()}</td>
            <td class="px-md py-sm font-code text-code">{uid_display}</td>
            <td class="px-md py-sm">
                {match page_link {
                    Some((href, short)) => view! { <a class="text-secondary hover:underline" href=href target="_blank">{short}</a> }.into_any(),
                    None => view! { "—" }.into_any(),
                }}
            </td>
            <td class="px-md py-sm">
                <span class=status_class>{status_label}</span>
                {detail_view}
            </td>
        </tr>
    }
}
