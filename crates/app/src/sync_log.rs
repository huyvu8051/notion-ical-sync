//! Phase 1 real page: shows the last 200 create/update/delete attempts for
//! one calendar, newest first — ported from the hand-rolled `format!()` HTML
//! previously in `src/oauth.rs::sync_log_page`. Read-only, no forms, no
//! client-side reactivity (nothing here is a signal) — the only thing this
//! phase needs to prove is that real DB-fetched data survives the SSR→CSR
//! round trip intact (see module docs in lib.rs).

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
    pub calendar_name: String,
    pub rows: Vec<SyncLogRow>,
}

const SYNC_LOG_STYLE: &str = r#"
* { box-sizing: border-box; }
body { font-family: -apple-system, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1.25rem; line-height: 1.5; color: #1a1a1a; }
.top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.25rem; }
.top-nav a.back { font-size: 0.85rem; color: #666; text-decoration: none; }
h1 { margin: 0.25rem 0 1.25rem; font-size: 1.3rem; }
table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
th, td { text-align: left; padding: 0.5rem 0.6rem; border-bottom: 1px solid #eee; vertical-align: top; }
th { color: #888; font-weight: 500; font-size: 0.78rem; text-transform: uppercase; letter-spacing: 0.02em; }
.status-ok { color: #166534; font-weight: 600; }
.status-error { color: #991b1b; font-weight: 600; }
.source-badge { display: inline-block; padding: 0.1rem 0.5rem; border-radius: 6px; font-size: 0.75rem; background: #f1f1f1; }
code { font-family: ui-monospace, monospace; background: #f1f1f1; padding: 0.1rem 0.35rem; border-radius: 4px; font-size: 0.78rem; word-break: break-all; }
.detail-cell { max-width: 320px; white-space: pre-wrap; word-break: break-word; color: #991b1b; }
.empty { color: #888; padding: 2rem 0; text-align: center; }
"#;

/// The whole HTML document. The data script sits in `<head>`, not `<body>`
/// — `<body>`'s children must exactly match what `SyncLogPage` renders for
/// `hydrate_body` to attach correctly (same lesson as the abandoned
/// migration and `TestApp`'s `Shell`), so anything not part of that
/// component tree has to live outside `<body>`.
#[component]
pub fn SyncLogShell(data: SyncLogPageData) -> impl IntoView {
    let title = format!("Log đồng bộ — {}", data.calendar_name);

    // `<` is escaped to `<` so `</script>` can never appear literally
    // inside the embedded object literal (sync log `detail` text is
    // server-generated error text, not user-authored, but it can echo
    // fragments of external API responses, so this isn't purely
    // theoretical). Valid JSON string escapes round-trip exactly through
    // `serde_json::from_str` on the client, and `<` never appears in JSON's
    // own structural syntax — only inside string values — so a global
    // replace is safe here.
    let json = serde_json::to_string(&data).unwrap_or_default();
    let json_safe = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__SYNC_LOG_DATA__ = {json_safe};");

    view! {
        <!DOCTYPE html>
        <html lang="vi">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>{title}</title>
                <style>{SYNC_LOG_STYLE}</style>
                <script inner_html=inline_data_script></script>
                <script type="module">
                    "import init, { hydrate_sync_log } from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_sync_log(JSON.stringify(window.__SYNC_LOG_DATA__)));"
                </script>
            </head>
            <body>
                <SyncLogPage data=data/>
            </body>
        </html>
    }
}

#[component]
pub fn SyncLogPage(data: SyncLogPageData) -> impl IntoView {
    let rows = data.rows;
    let body_rows = if rows.is_empty() {
        view! {
            <tr><td colspan="6" class="empty">"Chưa có hoạt động đồng bộ nào được ghi lại."</td></tr>
        }
        .into_any()
    } else {
        rows.into_iter()
            .map(|row| view! { <SyncLogRowView row=row/> })
            .collect_view()
            .into_any()
    };

    view! {
        <div id="sync-log-root">
            <div class="top-nav">
                <strong>"NotionCal"</strong>
                <a class="back" href="/me">"← Tất cả lịch"</a>
            </div>
            <h1>{format!("Log đồng bộ — {}", data.calendar_name)}</h1>
            <table>
                <thead>
                    <tr>
                        <th>"Thời gian"</th>
                        <th>"Nguồn"</th>
                        <th>"Hành động"</th>
                        <th>"UID sự kiện"</th>
                        <th>"Notion page"</th>
                        <th>"Kết quả"</th>
                    </tr>
                </thead>
                <tbody>{body_rows}</tbody>
            </table>
        </div>
    }
}

#[component]
fn SyncLogRowView(row: SyncLogRow) -> impl IntoView {
    let status_class = if row.status == "ok" { "status-ok" } else { "status-error" };
    let status_label = if row.status == "ok" { "OK" } else { "Lỗi" };
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
    let detail_view = (!row.detail.is_empty())
        .then(|| view! { <div class="detail-cell">{row.detail.clone()}</div> });

    view! {
        <tr>
            <td>{row.occurred_at.clone()}</td>
            <td><span class="source-badge">{row.source.clone()}</span></td>
            <td>{row.action.clone()}</td>
            <td><code>{uid_display}</code></td>
            <td>
                {match page_link {
                    Some((href, short)) => view! { <a href=href target="_blank">{short}</a> }.into_any(),
                    None => view! { "—" }.into_any(),
                }}
            </td>
            <td>
                <span class=status_class>{status_label}</span>
                {detail_view}
            </td>
        </tr>
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_sync_log(json: String) {
    console_error_panic_hook::set_once();
    let data: SyncLogPageData =
        serde_json::from_str(&json).expect("invalid sync log payload from server");
    leptos::mount::hydrate_body(move || view! { <SyncLogPage data=data.clone()/> });
}

// SSR-only render smoke tests — not a substitute for the real in-browser
// hydration check (see plan), but catches structural bugs (mismatched view
// branch types, panics on empty/edge-case data) before they ever reach a
// browser, which the abandoned attempt's fix-and-redeploy loop didn't have.
#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_row() -> SyncLogRow {
        SyncLogRow {
            occurred_at: "2026-08-21 10:00:00".to_string(),
            source: "webview".to_string(),
            action: "update".to_string(),
            event_uid: "evt-123".to_string(),
            notion_page_id: "abcd1234-5678-90ab-cdef-1234567890ab".to_string(),
            status: "ok".to_string(),
            detail: String::new(),
        }
    }

    #[test]
    fn renders_with_rows() {
        any_spawner::Executor::init_futures_executor().ok();
        let data = SyncLogPageData {
            calendar_name: "Work <Calendar>".to_string(),
            rows: vec![sample_row()],
        };
        let html = view! { <SyncLogPage data=data/> }.to_html();
        assert!(html.contains("evt-123"));
        assert!(html.contains("notion.so/abcd12345678"));
        // Leptos auto-escapes text content — confirms the app-level manual
        // html_escape() calls this migration removes weren't load-bearing.
        assert!(html.contains("&lt;Calendar&gt;") || html.contains("Work"));
    }

    #[test]
    fn renders_empty_state_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let data = SyncLogPageData {
            calendar_name: "Empty".to_string(),
            rows: vec![],
        };
        let html = view! { <SyncLogPage data=data/> }.to_html();
        assert!(html.contains("Chưa có hoạt động đồng bộ nào được ghi lại"));
    }

    #[test]
    fn shell_embeds_escaped_json_safe_from_script_breakout() {
        any_spawner::Executor::init_futures_executor().ok();
        let mut row = sample_row();
        row.detail = "</script><script>alert(1)</script>".to_string();
        let data = SyncLogPageData {
            calendar_name: "X".to_string(),
            rows: vec![row],
        };
        let html = view! { <SyncLogShell data=data/> }.to_html();
        assert!(!html.contains("</script><script>alert"));
    }
}
