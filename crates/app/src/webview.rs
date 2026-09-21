//! Phase B3 (final page) of the full Leptos SSR+CSR migration — the
//! `/app/{public_id}` calendar webview. FullCalendar (CDN JS, no Leptos
//! binding exists for it) stays exactly as-is per the user's explicit
//! decision: only the page *shell* (header, nav, static modal markup) is a
//! real Leptos SSR+hydrate tree; `webview_js()`'s content (FullCalendar
//! init, drag/drop, modal open/close/save, `fetch()` calls to the existing
//! CRUD routes) is unchanged and runs after hydration exactly as before.
//!
//! `webview_js()` is placed in `<head>` (not a `<body>`-trailing `<script>`
//! like the original) — safe because it already self-defers all DOM access:
//! the FullCalendar init is wrapped in its own
//! `document.addEventListener('DOMContentLoaded', ...)`, and every other
//! function (openCreateModal, saveFromModal, etc.) only touches the DOM
//! inside a function body invoked later by a click/callback, never at
//! top-level script-execution time. This also sidesteps ever needing to
//! decide whether a literal `<script>` tag inside a hydrated `<body>`
//! `inner_html` blob would be re-executed by hydration — untested territory
//! this migration doesn't need to risk.
//!
//! The modal's delete button is the one piece folded in from
//! `crates/islands::ConfirmActionButton` (see `confirm_button.rs`) — same
//! reasoning as `me.rs`'s `ConfirmButton`: it must be a real child of the
//! hydrated tree, so the header/modal markup around it is split into
//! `top_html`/`bottom_html` inner_html blobs with the button as a real node
//! in between, exactly the same shape `me.rs` uses per calendar card.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::confirm_button::ConfirmActionButton;

#[derive(Clone, Serialize, Deserialize)]
pub struct WebviewPageData {
    pub html_lang: String,
    pub title: String,
    /// Header through the modal's form fields, ending with the footer's
    /// opening `<div>` tag (up to, not including, the delete button).
    pub top_html: String,
    /// Rest of the modal footer (cancel/save buttons) + closing tags.
    pub bottom_html: String,
    pub delete_btn_label: String,
    pub confirm_delete_event_label: String,
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
    let top_html = data.top_html.clone();
    let bottom_html = data.bottom_html.clone();

    view! {
        <div id="webview-root">
            <div inner_html=top_html></div>
            <ConfirmActionButton
                id="modal-delete-btn".to_string()
                label=data.delete_btn_label
                confirm_label=data.confirm_delete_event_label
                class="text-error text-label-md hover:underline hidden".to_string()
                on_confirm_fn="deleteFromModal".to_string()
            />
            <div inner_html=bottom_html></div>
        </div>
    }
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
            top_html: "<header>top</header><div id=\"calendar\"></div>".to_string(),
            bottom_html: "<div>bottom</div>".to_string(),
            delete_btn_label: "Xoá".to_string(),
            confirm_delete_event_label: "Xoá sự kiện này?".to_string(),
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
