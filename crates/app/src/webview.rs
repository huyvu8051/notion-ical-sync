//! Phase B3 (final page) of the full Leptos SSR+CSR migration — the
//! `/app/{public_id}` calendar webview. FullCalendar (CDN JS, no Leptos
//! binding exists for it) stays exactly as-is per the user's explicit
//! decision: only the page *shell* is a real Leptos SSR+hydrate tree;
//! `webview_js()`'s content (FullCalendar init, drag/drop, modal
//! open/close/save, `fetch()` calls to the existing CRUD routes) is
//! unchanged and runs after hydration exactly as before.
//!
//! **No `crates/islands::ConfirmActionButton` fold-in here, unlike `me.rs`'s
//! `ConfirmButton`.** An earlier version of this file did fold it in, and
//! hit a real, browser-verified (real Keycloak+Postgres stack, not just SSR
//! unit tests) `tachys::hydration::failed_to_cast_element` /
//! `failed_to_cast_text_node` panic. An isolated experiment — swapping
//! `ConfirmActionButton` for a plain static `<button>` with everything else
//! identical — made the panic disappear, confirming the component itself
//! (not surrounding DOM structure, which was independently verified
//! correct via `DOMParser`) was the cause; the exact root cause inside
//! tachys wasn't chased further, since the delete-confirm button doesn't
//! actually need real Rust-side reactivity at all. It's reimplemented as
//! plain JS in `webview_js()` (`handleDeleteClick`, same "click to arm, 3s
//! window to confirm" UX as `ConfirmButton`/`ConfirmActionButton`), which
//! means *nothing* on this page needs Leptos-managed state, so the whole
//! body collapses to one `inner_html` blob — same shape as
//! `legal.rs`/`connect_notion.rs`, not `me.rs`.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct WebviewPageData {
    pub html_lang: String,
    pub title: String,
    /// The whole `<body>` content — header, `#calendar` div, and the modal
    /// (including its delete/cancel/save buttons) — fully self-contained.
    pub body_html: String,
    /// `webview_js()`'s output, unchanged — see module doc for why this is
    /// safe to place in `<head>` rather than a `<body>`-trailing script.
    pub js: String,
}

const WEBVIEW_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.modal-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
#calendar { max-width: 1100px; margin: 0 auto; padding: 24px; }
.fc { --fc-border-color: #e5e5e5; --fc-button-bg-color: #fff; --fc-button-border-color: #e5e5e5; --fc-button-text-color: #1b1c1c;
  --fc-button-active-bg-color: #000; --fc-button-active-border-color: #000; --fc-today-bg-color: #f5f3f3; font-family: 'Inter', sans-serif; }
.fc .fc-button { box-shadow: none !important; text-transform: none; font-weight: 500; }
@media (max-width: 640px) {
  #calendar { padding: 12px; }
  .fc-header-toolbar { flex-wrap: wrap; row-gap: 8px; justify-content: center !important; }
  .fc-toolbar-chunk { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px; }
  .fc-toolbar-title { font-size: 1.1em !important; }
  .fc .fc-button { padding: 4px 8px !important; font-size: 0.8em !important; }
  header.h-16 { padding-left: 12px; padding-right: 12px; }
  header .text-h1 { font-size: 18px; max-width: 40vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
}
@media (max-width: 420px) {
  header .back-label { display: none; }
}
"#;
const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

/// The whole HTML document. `<head>` is one `inner_html` blob (never part of
/// `hydrate_body`'s reconciliation — see the other Phase A/B pages) holding
/// the FullCalendar CDN tags, styles, `webview_js()`, and the usual data
/// script + hydrate bootstrap.
#[component]
pub fn WebviewShell(data: WebviewPageData) -> impl IntoView {
    let html_lang = data.html_lang.clone();

    let json = serde_json::to_string(&data).unwrap_or_default();
    let json_safe = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__WEBVIEW_DATA__ = {json_safe};");

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} — NotionCal</title><link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.css"><script src="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js"></script><link rel="stylesheet" href="/assets/style-webview.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{js}</script><script>{data_script}</script><script type="module">import init, {{ hydrate_webview }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_webview(JSON.stringify(window.__WEBVIEW_DATA__)));</script>"#,
        title = data.title,
        fonts = GOOGLE_FONTS_HREF,
        style = WEBVIEW_HEAD_STYLE,
        js = data.js,
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
            // Pre-escaped, as the real caller (src/webview.rs) always sends it.
            title: "Work &lt;Calendar&gt;".to_string(),
            body_html: "<header>top</header><div id=\"calendar\"></div><button id=\"modal-delete-btn\">Xoá</button>".to_string(),
            js: "console.log('webview js');".to_string(),
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
    fn shell_embeds_js_and_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <WebviewShell data=sample_data()/> }.to_html());
        assert!(html.contains("console.log('webview js')"));
        assert!(html.contains("fullcalendar@6.1.15"));
        // `title` lands in `head_html` as raw text (head isn't a real view!
        // tree, so nothing auto-escapes it) — the caller (src/webview.rs)
        // is responsible for pre-escaping it, which `sample_data()` mimics
        // by using an already-`&lt;`/`&gt;`-escaped value.
        assert!(html.contains("&lt;Calendar&gt;"));
        assert!(!html.contains("</script><script>alert"));
    }
}
