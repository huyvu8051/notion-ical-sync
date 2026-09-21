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

fn pkg_js_and_wasm_file_names(options: &leptos::config::LeptosOptions) -> (String, String) {
    let js_file_name = options.output_name.to_string();
    let mut wasm_file_name = options.output_name.to_string();
    let compiled_under_cargo_leptos = option_env!("LEPTOS_OUTPUT_NAME").is_some();
    if !compiled_under_cargo_leptos {
        wasm_file_name.push_str("_bg");
    }
    (js_file_name, wasm_file_name)
}

#[component]
pub fn SyncLogShell(data: SyncLogPageData, leptos_options: leptos::config::LeptosOptions) -> impl IntoView {
    let html_lang = data.html_lang.clone();
    let title = format!("{} — {}", data.heading_label, data.calendar_name);

    let json = serde_json::to_string(&data).unwrap_or_default();
    let script_breakout_safe_json = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__SYNC_LOG_DATA__ = {script_breakout_safe_json};");
    let (js_file_name, wasm_file_name) = pkg_js_and_wasm_file_names(&leptos_options);

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title><link rel="stylesheet" href="/assets/style-auth-a.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{data_script}</script><script type="module">import init, {{ hydrate_sync_log }} from '/pkg/{js_file_name}.js'; init('/pkg/{wasm_file_name}.wasm').then(() => hydrate_sync_log(JSON.stringify(window.__SYNC_LOG_DATA__)));</script>"#,
        fonts = crate::page_shell::GOOGLE_FONTS_HREF,
        style = crate::page_shell::ONBOARDING_HEAD_STYLE,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang=html_lang>
            <head inner_html=head_html></head>
            <body class="bg-background text-on-surface font-body-md min-h-screen">
                <SyncLogPage data=data/>
            </body>
        </html>
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
                    <a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/me">
                        <span class="material-symbols-outlined">arrow_back</span>
                    </a>
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

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_sync_log(json: String) {
    console_error_panic_hook::set_once();
    let data: SyncLogPageData =
        serde_json::from_str(&json).expect("invalid sync log payload from server");
    leptos::mount::hydrate_body(move || view! { <SyncLogPage data=data.clone()/> });
}

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

    fn sample_data(rows: Vec<SyncLogRow>) -> SyncLogPageData {
        SyncLogPageData {
            html_lang: "vi".to_string(),
            top_nav_html: "<header>nav</header>".to_string(),
            calendar_name: "Work <Calendar>".to_string(),
            heading_label: "Log đồng bộ".to_string(),
            col_time: "Thời gian".to_string(),
            col_source: "Nguồn".to_string(),
            col_action: "Hành động".to_string(),
            col_event_uid: "UID sự kiện".to_string(),
            col_notion_page: "Notion page".to_string(),
            col_result: "Kết quả".to_string(),
            empty_state: "Chưa có hoạt động đồng bộ nào được ghi lại.".to_string(),
            status_ok: "OK".to_string(),
            status_error: "Lỗi".to_string(),
            rows,
        }
    }

    #[test]
    fn renders_with_rows() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <SyncLogPage data=sample_data(vec![sample_row()])/> }.to_html());
        assert!(html.contains("evt-123"));
        assert!(html.contains("notion.so/abcd12345678"));
        assert!(html.contains("&lt;Calendar&gt;") || html.contains("Work"));
    }

    #[test]
    fn renders_empty_state_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <SyncLogPage data=sample_data(vec![])/> }.to_html());
        assert!(html.contains("Chưa có hoạt động đồng bộ nào được ghi lại"));
    }

    #[test]
    fn shell_embeds_escaped_json_safe_from_script_breakout() {
        any_spawner::Executor::init_futures_executor().ok();
        let mut row = sample_row();
        row.detail = "</script><script>alert(1)</script>".to_string();
        let leptos_options = leptos::config::LeptosOptions::builder()
            .output_name("app")
            .build();
        let html = leptos::prelude::Owner::new().with(|| {
            view! { <SyncLogShell data=sample_data(vec![row]) leptos_options=leptos_options/> }
                .to_html()
        });
        assert!(!html.contains("</script><script>alert"));
    }
}
