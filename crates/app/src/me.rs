use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::confirm_button::ConfirmButton;

#[derive(Clone, Serialize, Deserialize)]
pub struct CalendarCardData {
    pub public_id: String,
    pub label: String,
    pub active_badge: String,
    pub open_calendar_label: String,
    pub url_row_html: String,
    pub username_row_html: String,
    pub password_row_html: String,
    pub paste_hint: String,
    pub regenerate_password_label: String,
    pub regenerate_confirm_label: String,
    pub regenerate_action: String,
    pub view_log_label: String,
    pub view_log_href: String,
    pub delete_label: String,
    pub delete_confirm_label: String,
    pub delete_action: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MePageData {
    pub html_lang: String,
    pub page_title: String,
    pub header_html: String,
    pub main_top_html: String,
    pub main_bottom_html: String,
    pub calendars: Vec<CalendarCardData>,
    pub empty_state_html: String,
}

const DASHBOARD_HEAD_STYLE: &str = r#"
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.success-banner-gradient { background: linear-gradient(90deg, rgba(220, 252, 231, 0.5) 0%, rgba(220, 252, 231, 0.2) 100%); }
.error-banner-gradient { background: linear-gradient(90deg, rgba(254, 226, 226, 0.5) 0%, rgba(254, 226, 226, 0.2) 100%); }
"#;

use crate::page_shell::GOOGLE_FONTS_HREF;

const COPY_TO_CLIPBOARD_JS: &str = r#"
function copyToClipboard(text, btn) {
  navigator.clipboard.writeText(text).then(() => {
    const icon = btn.querySelector('.material-symbols-outlined');
    const original = icon.innerText;
    icon.innerText = 'check';
    icon.classList.add('text-[#166534]');
    setTimeout(() => { icon.innerText = original; icon.classList.remove('text-[#166534]'); }, 2000);
  });
}
"#;

#[component]
pub fn MeShell(data: MePageData) -> impl IntoView {
    let html_lang = data.html_lang.clone();

    let json = serde_json::to_string(&data).unwrap_or_default();
    let script_breakout_safe_json = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__ME_DATA__ = {script_breakout_safe_json};");

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title><link rel="stylesheet" href="/assets/style-auth-a.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{copy_js}</script><script>{data_script}</script><script type="module">import init, {{ hydrate_me }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_me(JSON.stringify(window.__ME_DATA__)));</script>"#,
        title = data.page_title,
        fonts = GOOGLE_FONTS_HREF,
        style = DASHBOARD_HEAD_STYLE,
        copy_js = COPY_TO_CLIPBOARD_JS,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang=html_lang>
            <head inner_html=head_html></head>
            <body class="bg-background text-on-surface font-body-md min-h-screen">
                <MePage data=data/>
            </body>
        </html>
    }
}

#[component]
pub fn MePage(data: MePageData) -> impl IntoView {
    let header_html = data.header_html.clone();
    let main_top_html = data.main_top_html.clone();
    let main_bottom_html = data.main_bottom_html.clone();

    let list = if data.calendars.is_empty() {
        view! { <div inner_html=data.empty_state_html.clone()></div> }.into_any()
    } else {
        data.calendars
            .into_iter()
            .map(|c| view! { <CalendarCard data=c/> })
            .collect_view()
            .into_any()
    };

    view! {
        <div id="me-root">
            <div inner_html=header_html></div>
            <main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
                <div inner_html=main_top_html></div>
                <div class="space-y-md">{list}</div>
                <div inner_html=main_bottom_html></div>
            </main>
        </div>
    }
}

#[component]
fn CalendarCard(data: CalendarCardData) -> impl IntoView {
    let open_href = format!("/app/{}", data.public_id);

    view! {
        <div class="bg-surface border border-outline-variant rounded-lg p-lg hover:border-outline transition-colors duration-200">
            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-md mb-lg">
                <div class="flex items-center gap-sm">
                    <span class="text-h2">"🗓️"</span>
                    <h2 class="font-semibold text-h2">{data.label}</h2>
                    <span class="bg-[#DCFCE7] text-[#166534] px-xs py-[2px] rounded font-label-md text-[10px] uppercase tracking-wider">{data.active_badge}</span>
                </div>
                <a class="px-md h-8 border border-outline-variant hover:bg-surface-container-low font-label-md text-label-md transition-all flex items-center" href=open_href>{data.open_calendar_label}</a>
            </div>
            <div inner_html=data.url_row_html></div>
            <div inner_html=data.username_row_html></div>
            {(!data.password_row_html.is_empty()).then(|| view! { <div inner_html=data.password_row_html.clone()></div> })}
            <p class="text-on-surface-variant text-[13px] mt-sm">{data.paste_hint}</p>
            <div class="flex items-center gap-md mt-md pt-md border-t border-outline-variant">
                <ConfirmButton
                    action=data.regenerate_action
                    label=data.regenerate_password_label
                    confirm_label=data.regenerate_confirm_label
                    class="text-label-md text-secondary hover:underline".to_string()
                />
                <a href=data.view_log_href class="text-label-md text-secondary hover:underline">{data.view_log_label}</a>
                <div class="ml-auto">
                    <ConfirmButton
                        action=data.delete_action
                        label=data.delete_label
                        confirm_label=data.delete_confirm_label
                        class="text-label-md text-error hover:underline".to_string()
                    />
                </div>
            </div>
        </div>
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_me(json: String) {
    console_error_panic_hook::set_once();
    let data: MePageData =
        serde_json::from_str(&json).expect("invalid /me page payload from server");
    leptos::mount::hydrate_body(move || view! { <MePage data=data.clone()/> });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_card() -> CalendarCardData {
        CalendarCardData {
            public_id: "abc123".to_string(),
            label: "Work <Cal>".to_string(),
            active_badge: "Đang hoạt động".to_string(),
            open_calendar_label: "Mở lịch".to_string(),
            url_row_html: "<div>url row</div>".to_string(),
            username_row_html: "<div>username row</div>".to_string(),
            password_row_html: String::new(),
            paste_hint: "paste hint".to_string(),
            regenerate_password_label: "Tạo lại mật khẩu".to_string(),
            regenerate_confirm_label: "Tạo mật khẩu mới?".to_string(),
            regenerate_action: "/me/calendars/abc123/regenerate-password".to_string(),
            view_log_label: "Xem log đồng bộ".to_string(),
            view_log_href: "/me/calendars/abc123/log".to_string(),
            delete_label: "Xoá".to_string(),
            delete_confirm_label: "Xoá calendar này?".to_string(),
            delete_action: "/me/calendars/abc123/delete".to_string(),
        }
    }

    fn sample_data(calendars: Vec<CalendarCardData>) -> MePageData {
        MePageData {
            html_lang: "vi".to_string(),
            page_title: "Trang của bạn — NotionCal".to_string(),
            header_html: "<header>top</header>".to_string(),
            main_top_html: "<div>banners+billing+heading</div>".to_string(),
            main_bottom_html: "<footer>bottom</footer>".to_string(),
            calendars,
            empty_state_html: "<p>Chưa có calendar nào</p>".to_string(),
        }
    }

    #[test]
    fn renders_calendar_cards_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <MePage data=sample_data(vec![sample_card()])/> }.to_html());
        assert!(html.contains("abc123"));
        assert!(html.contains("/me/calendars/abc123/regenerate-password"));
        assert!(html.contains("/me/calendars/abc123/delete"));
        assert!(html.contains("&lt;Cal&gt;") || html.contains("Work"));
    }

    #[test]
    fn renders_empty_state_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <MePage data=sample_data(vec![])/> }.to_html());
        assert!(html.contains("Chưa có calendar nào"));
    }

    #[test]
    fn shell_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <MeShell data=sample_data(vec![sample_card()])/> }.to_html());
        assert!(!html.contains("</script><script>alert"));
    }
}
