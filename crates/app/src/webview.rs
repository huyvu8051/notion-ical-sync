use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebviewJsConfig {
    pub events_url: String,
    pub modal_title_add: String,
    pub modal_title_edit: String,
    pub alert_enter_title: String,
    pub alert_pick_start: String,
    pub alert_update_failed: String,
    pub alert_create_failed: String,
    pub alert_delete_failed: String,
    pub alert_update_date_failed: String,
    pub alert_quota_exceeded: String,
    pub repeat_display_prefix: String,
    pub attendees_display_prefix: String,
    pub delete_btn: String,
    pub confirm_delete_event: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WebviewPageData {
    pub html_lang: String,
    pub title: String,
    pub body_html: String,
    pub js_config: WebviewJsConfig,
}

const WEBVIEW_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.modal-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
#calendar { max-width: 1100px; margin: 0 auto; padding: 24px; }
.fc { --fc-border-color: #c4c7c7; --fc-button-bg-color: #fff; --fc-button-border-color: #c4c7c7; --fc-button-text-color: #1b1c1c;
  --fc-button-active-bg-color: #000; --fc-button-active-border-color: #000; --fc-today-bg-color: #f5f3f3; font-family: 'Inter', sans-serif; }
.fc .fc-button { box-shadow: none !important; text-transform: none; font-weight: 500; }
@media (max-width: 640px) {
  #calendar { padding: 12px; }
  .fc-header-toolbar { flex-wrap: wrap; row-gap: 8px; justify-content: center !important; }
  .fc-toolbar-chunk { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px; }
  .fc-toolbar-title { font-size: 1.1em !important; }
  .fc .fc-button { padding: 4px 8px !important; font-size: 0.8em !important; }
  header.h-16 { padding-left: 12px; padding-right: 12px; }
  header.h-16 .text-h1 { font-size: 18px; max-width: 40vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
}
@media (max-width: 420px) {
  header.h-16 .back-label { display: none; }
}
"#;
const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

#[component]
pub fn WebviewShell(data: WebviewPageData) -> impl IntoView {
    let html_lang = data.html_lang.clone();

    let json = serde_json::to_string(&data).unwrap_or_default();
    let json_safe = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__WEBVIEW_DATA__ = {json_safe};");

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} — NotionCal</title><link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.css"><script src="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js"></script><link rel="stylesheet" href="/assets/style-auth-a.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{data_script}</script><script src="/static/webview.js" defer></script><script type="module">import init, {{ hydrate_webview }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_webview(JSON.stringify(window.__WEBVIEW_DATA__)));</script>"#,
        title = data.title,
        fonts = GOOGLE_FONTS_HREF,
        style = WEBVIEW_HEAD_STYLE,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang=html_lang>
            <head inner_html=head_html></head>
            <body class="bg-background text-on-surface">
                <WebviewPage data=data/>
            </body>
        </html>
    }
}

#[component]
pub fn WebviewPage(data: WebviewPageData) -> impl IntoView {
    view! { <div id="webview-root" inner_html=data.body_html></div> }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_webview(json: String) {
    console_error_panic_hook::set_once();
    let data: WebviewPageData =
        serde_json::from_str(&json).expect("invalid webview page payload from server");
    leptos::mount::hydrate_body(move || view! { <WebviewPage data=data.clone()/> });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_data() -> WebviewPageData {
        WebviewPageData {
            html_lang: "vi".to_string(),
            title: "Work &lt;Calendar&gt;".to_string(),
            body_html: "<header>top</header><div id=\"calendar\"></div><button id=\"modal-delete-btn\">Xoá</button>".to_string(),
            js_config: WebviewJsConfig {
                events_url: "/app/test-id/api/events".to_string(),
                modal_title_add: "Thêm sự kiện".to_string(),
                modal_title_edit: "Chỉnh sửa sự kiện".to_string(),
                alert_enter_title: "Nhập tên sự kiện".to_string(),
                alert_pick_start: "Chọn ngày bắt đầu".to_string(),
                alert_update_failed: "Cập nhật thất bại".to_string(),
                alert_create_failed: "Tạo event thất bại".to_string(),
                alert_delete_failed: "Xoá thất bại".to_string(),
                alert_update_date_failed: "Cập nhật ngày thất bại".to_string(),
                alert_quota_exceeded: "Đã đạt giới hạn".to_string(),
                repeat_display_prefix: "Lặp lại: ".to_string(),
                attendees_display_prefix: "Người được mời: ".to_string(),
                delete_btn: "Xoá".to_string(),
                confirm_delete_event: "Xoá sự kiện này?".to_string(),
            },
        }
    }

    #[test]
    fn renders_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <WebviewPage data=sample_data()/> }.to_html());
        assert!(html.contains("modal-delete-btn"));
        assert!(html.contains("id=\"calendar\""));
    }

    #[test]
    fn shell_embeds_js_config_and_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <WebviewShell data=sample_data()/> }.to_html());
        assert!(html.contains("/app/test-id/api/events"));
        assert!(html.contains("/static/webview.js"));
        assert!(html.contains("fullcalendar@6.1.15"));
        assert!(html.contains("&lt;Calendar&gt;"));
        assert!(!html.contains("</script><script>alert"));
    }
}
