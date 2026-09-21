//! Phase A (static batch) of the full Leptos SSR+CSR migration — the
//! "Connect your Notion workspace" step-1 confirmation screen at
//! `/connect/notion`. Same `inner_html`-blob shape as `legal.rs`/`landing.rs`:
//! pure static content per (email, language) pair, zero client interactivity,
//! `<head>` gets no hydration benefit from being real view nodes since only
//! `<body>` is ever hydrated (see `landing.rs`'s module doc for the full
//! reasoning, including why `<head>` — data script + hydrate bootstrap
//! included — lives in one blob rather than sibling `view!` elements).
//! `top_nav_html` arrives pre-rendered from the caller (`oauth.rs`'s existing
//! `onboarding_top_nav()`, still shared with the not-yet-migrated
//! `pick_databases_page`) rather than being duplicated here.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectNotionPageData {
    pub html_lang: String,
    pub title: String,
    pub top_nav_html: String,
    pub heading: String,
    pub body: String,
    pub connect_cta: String,
    pub bullet_read_write: String,
    pub bullet_disconnect: String,
    pub bullet_no_sharing: String,
    pub privacy_link: String,
    pub terms_link: String,
}

// Shared with `pick_databases.rs` (the other onboarding-flow page) — public
// within the crate so that module can reuse it instead of a third copy.
// Originally duplicated from `oauth.rs::ONBOARDING_HEAD`, which stays there
// only as long as any not-yet-migrated page still references it.
pub(crate) const ONBOARDING_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
.custom-checkbox:checked { background-color: #000000; border-color: #000000; }
.card-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
"#;
pub(crate) const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

/// The whole HTML document. `<head>` (see module doc) and `<body>`'s single
/// `ConnectNotionPage` child both come from `data`.
#[component]
pub fn ConnectNotionShell(data: ConnectNotionPageData) -> impl IntoView {
    let html_lang = data.html_lang.clone();

    let json = serde_json::to_string(&data).unwrap_or_default();
    let json_safe = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__CONNECT_NOTION_DATA__ = {json_safe};");

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} — NotionCal</title><link rel="stylesheet" href="/assets/style-oauth.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{data_script}</script><script type="module">import init, {{ hydrate_connect_notion }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_connect_notion(JSON.stringify(window.__CONNECT_NOTION_DATA__)));</script>"#,
        title = data.title,
        fonts = GOOGLE_FONTS_HREF,
        style = ONBOARDING_HEAD_STYLE,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang=html_lang>
            <head inner_html=head_html></head>
            <body class="min-h-screen flex flex-col">
                <ConnectNotionPage data=data/>
            </body>
        </html>
    }
}

#[component]
pub fn ConnectNotionPage(data: ConnectNotionPageData) -> impl IntoView {
    let body_html = format!(
        r##"{top_nav}
<main class="flex-grow flex items-center justify-center pt-[56px] px-margin-mobile md:px-margin-desktop">
<div class="w-full max-w-[480px] bg-surface-container-lowest border border-outline-variant p-xl rounded-lg card-shadow">
<div class="flex justify-center items-center gap-md mb-lg">
<div class="w-12 h-12 flex items-center justify-center bg-surface-container border border-outline-variant rounded-xl">
<span class="material-symbols-outlined text-[28px]">link</span>
</div>
<div class="w-2 h-[1px] bg-outline-variant"></div>
<div class="w-12 h-12 flex items-center justify-center bg-primary rounded-xl">
<svg class="w-7 h-7 text-white fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
</div>
</div>
<div class="text-center mb-xl">
<h1 class="text-h1 mb-sm tracking-tight text-primary">{heading}</h1>
<p class="text-body-md text-on-surface-variant leading-relaxed">{body}</p>
</div>
<a class="w-full h-12 bg-primary text-white text-label-md flex items-center justify-center gap-sm rounded-lg hover:bg-zinc-800 transition-all active:scale-[0.98] mb-lg" href="/connect/notion/start">
<svg class="w-5 h-5 fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
{connect_cta}
</a>
<div class="border-t border-outline-variant pt-lg space-y-md">
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_read_write}</span>
</div>
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_disconnect}</span>
</div>
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_no_sharing}</span>
</div>
</div>
<div class="mt-xl text-center">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors underline underline-offset-4" href="/privacy">{privacy_link}</a>
<span class="text-outline-variant mx-2">·</span>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors underline underline-offset-4" href="/terms">{terms_link}</a>
</div>
</div>
</main>"##,
        top_nav = data.top_nav_html,
        heading = data.heading,
        body = data.body,
        connect_cta = data.connect_cta,
        bullet_read_write = data.bullet_read_write,
        bullet_disconnect = data.bullet_disconnect,
        bullet_no_sharing = data.bullet_no_sharing,
        privacy_link = data.privacy_link,
        terms_link = data.terms_link,
    );

    view! {
        <div id="connect-notion-root" inner_html=body_html></div>
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_connect_notion(json: String) {
    console_error_panic_hook::set_once();
    let data: ConnectNotionPageData =
        serde_json::from_str(&json).expect("invalid connect-notion page payload from server");
    leptos::mount::hydrate_body(move || view! { <ConnectNotionPage data=data.clone()/> });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_data() -> ConnectNotionPageData {
        ConnectNotionPageData {
            html_lang: "vi".to_string(),
            title: "Kết nối Notion".to_string(),
            top_nav_html: "<header>nav</header>".to_string(),
            heading: "Kết nối không gian làm việc Notion của bạn".to_string(),
            body: "Chúng tôi cần quyền truy cập...".to_string(),
            connect_cta: "Kết nối với Notion".to_string(),
            bullet_read_write: "Chỉ đọc và ghi vào các trang bạn cho phép".to_string(),
            bullet_disconnect: "Có thể ngắt kết nối bất cứ lúc nào".to_string(),
            bullet_no_sharing: "Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba".to_string(),
            privacy_link: "Chính sách bảo mật".to_string(),
            terms_link: "Điều khoản dịch vụ".to_string(),
        }
    }

    #[test]
    fn renders_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = view! { <ConnectNotionPage data=sample_data()/> }.to_html();
        assert!(html.contains("Kết nối với Notion"));
        assert!(html.contains("/connect/notion/start"));
    }

    #[test]
    fn shell_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = view! { <ConnectNotionShell data=sample_data()/> }.to_html();
        assert!(!html.contains("</script><script>alert"));
    }
}
