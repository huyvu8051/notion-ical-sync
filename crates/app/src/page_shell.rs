use leptos::html;
use leptos::prelude::*;

pub(crate) const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

pub(crate) const ROOT_ICON_STYLE: &str =
    ".material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }";

pub(crate) const HOME_HEADER_STYLE: &str =
    ".glass-header { backdrop-filter: blur(8px); background: rgba(251, 249, 249, 0.85); }";

pub(crate) const ONBOARDING_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.custom-checkbox:checked { background-color: #000000; border-color: #000000; }
.card-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
"#;

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct NotionApiBaseUrl(pub String);

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct AppBaseUrl(pub String);

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct StripeConfigured(pub bool);

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct MapboxToken(pub Option<String>);

#[cfg(feature = "ssr")]
pub(crate) fn query_param_i64(name: &str) -> Option<i64> {
    let parts = leptos::prelude::use_context::<axum::http::request::Parts>()?;
    let query = parts.uri.query()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == name {
            return v.parse().ok();
        }
    }
    None
}

#[cfg(not(feature = "ssr"))]
pub(crate) fn query_param_i64(name: &str) -> Option<i64> {
    let search = web_sys::window()?.location().search().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == name {
            return v.parse().ok();
        }
    }
    None
}

#[cfg(feature = "ssr")]
pub(crate) fn current_user_email() -> String {
    leptos::prelude::use_context::<axum::http::request::Parts>()
        .and_then(|parts| {
            parts
                .extensions
                .get::<axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims>>()
                .cloned()
        })
        .and_then(|claims| claims.email().map(|e| e.as_str().to_string()))
        .unwrap_or_default()
}

pub(crate) const CLIENT_TZ_SPAN_HTML: &str =
    r#"<span id="client-tz" class="text-label-md text-on-surface-variant"></span>"#;

#[cfg(feature = "hydrate")]
pub(crate) fn install_client_timezone_label() {
    leptos::prelude::Effect::new(move |_| {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(el) = document.get_element_by_id("client-tz") else {
            return;
        };
        let label = js_sys::eval(
            r#"(function() {
                var tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
                var offsetMin = -new Date().getTimezoneOffset();
                var sign = offsetMin >= 0 ? '+' : '-';
                var abs = Math.abs(offsetMin);
                var hours = Math.floor(abs / 60);
                var minutes = abs % 60;
                var utc = 'UTC' + sign + hours + (minutes ? ':' + String(minutes).padStart(2, '0') : '');
                return tz + ' (' + utc + ')';
            })()"#,
        )
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
        el.set_text_content(Some(&label));
    });
}

#[cfg(feature = "hydrate")]
pub(crate) fn install_client_local_time_labels() {
    leptos::prelude::Effect::new(move |_| {
        let _ = js_sys::eval(
            r#"(function() {
                var pad = function(n) { return String(n).padStart(2, '0'); };
                document.querySelectorAll('.occurred-at[data-utc]').forEach(function(el) {
                    var d = new Date(el.getAttribute('data-utc'));
                    if (isNaN(d.getTime())) { return; }
                    el.textContent = d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate())
                        + ' ' + pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds());
                });
            })()"#,
        );
    });
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn top_nav_html(email: &str) -> String {
    format!(
        r#"<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
<div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
<a href="/" class="text-h1 font-semibold tracking-tighter text-primary hover:opacity-70 transition-opacity">NotionCal</a>
<div class="flex items-center space-x-md">
<span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
<a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="Log out">
<span class="material-symbols-outlined">logout</span>
</a>
</div>
</div>
</header>"#,
        email = html_escape(email),
    )
}

pub(crate) fn footer_html() -> String {
    format!(
        r#"<p class="text-on-surface-variant text-[13px] pt-lg">
{client_tz}
<span class="px-2">·</span>
<a class="underline hover:text-primary" href="/privacy">Privacy Policy</a>
<span class="px-2">·</span>
<a class="underline hover:text-primary" href="/terms">Terms of Service</a>
</p>"#,
        client_tz = CLIENT_TZ_SPAN_HTML,
    )
}

// --- Real Leptos components (Phase A of the inner_html/JS reduction pass) ---
// These replace the string-builder + imperative-DOM-patch functions above at
// call sites one at a time; both coexist until every page has migrated.

#[cfg(feature = "hydrate")]
fn compute_client_timezone_label() -> String {
    let dtf = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new());
    let resolved = dtf.resolved_options();
    let tz = js_sys::Reflect::get(&resolved, &wasm_bindgen::JsValue::from_str("timeZone"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    let offset_min = -js_sys::Date::new_0().get_timezone_offset();
    let sign = if offset_min >= 0.0 { "+" } else { "-" };
    let abs = offset_min.abs();
    let hours = (abs / 60.0).floor() as i64;
    let minutes = (abs % 60.0) as i64;
    let utc = if minutes != 0 {
        format!("UTC{sign}{hours}:{minutes:02}")
    } else {
        format!("UTC{sign}{hours}")
    };
    format!("{tz} ({utc})")
}

/// Client-only: the server has no way to know the visitor's browser
/// timezone. Uses a `NodeRef` (populated only after real mount) instead of
/// `document.get_element_by_id`, so there's no race with `inner_html`
/// insertion timing — the bug that motivated this whole pass.
#[component]
pub fn ClientTimezone(#[prop(optional, into)] class: String) -> impl IntoView {
    let node_ref: NodeRef<html::Span> = NodeRef::new();
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(el) = node_ref.get() {
            el.set_text_content(Some(&compute_client_timezone_label()));
        }
    });
    view! { <span node_ref=node_ref class=class></span> }
}

#[cfg(feature = "hydrate")]
fn format_local_time(utc: &str) -> Option<String> {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(utc));
    if date.get_time().is_nan() {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds(),
    ))
}

/// Renders `utc` (an ISO-8601 UTC timestamp) as-is on first paint, then
/// swaps in the visitor's local wall-clock time client-side.
#[component]
pub fn LocalTime(utc: String) -> impl IntoView {
    let node_ref: NodeRef<html::Span> = NodeRef::new();
    #[cfg(feature = "hydrate")]
    {
        let utc = utc.clone();
        Effect::new(move |_| {
            if let (Some(el), Some(formatted)) = (node_ref.get(), format_local_time(&utc)) {
                el.set_text_content(Some(&formatted));
            }
        });
    }
    view! { <span node_ref=node_ref>{utc}</span> }
}

#[component]
pub fn TopNav(email: String) -> impl IntoView {
    view! {
        <header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
            <div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
                <a href="/" class="text-h1 font-semibold tracking-tighter text-primary hover:opacity-70 transition-opacity">"NotionCal"</a>
                <div class="flex items-center space-x-md">
                    <span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
                    <a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="Log out">
                        <span class="material-symbols-outlined">"logout"</span>
                    </a>
                </div>
            </div>
        </header>
    }
}

#[component]
pub fn PageFooter() -> impl IntoView {
    view! {
        <p class="text-on-surface-variant text-[13px] pt-lg">
            <ClientTimezone class="text-label-md text-on-surface-variant".to_string()/>
            <span class="px-2">"·"</span>
            <a class="underline hover:text-primary" href="/privacy">"Privacy Policy"</a>
            <span class="px-2">"·"</span>
            <a class="underline hover:text-primary" href="/terms">"Terms of Service"</a>
        </p>
    }
}

#[component]
pub fn HomeHeader() -> impl IntoView {
    view! {
        <header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
                <div class="flex items-center gap-8">
                    <a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
                        <span class="material-symbols-outlined text-primary">"calendar_month"</span>
                        "NotionCal"
                    </a>
                    <nav class="hidden md:flex items-center gap-6">
                        <a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#how-it-works">"How it works"</a>
                        <a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#pricing">"Pricing"</a>
                    </nav>
                </div>
                <div class="flex items-center gap-4">
                    <a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me" rel="external">"Log in / Sign up"</a>
                </div>
            </div>
        </header>
    }
}

#[cfg(feature = "hydrate")]
async fn copy_to_clipboard_and_flash_icon(text: String, icon: web_sys::Element) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let promise = window.navigator().clipboard().write_text(&text);
    if wasm_bindgen_futures::JsFuture::from(promise).await.is_err() {
        return;
    }
    let original = icon.text_content().unwrap_or_default();
    icon.set_text_content(Some("check"));
    let _ = icon.class_list().add_1("text-[#166534]");
    gloo_timers::future::TimeoutFuture::new(2000).await;
    icon.set_text_content(Some(&original));
    let _ = icon.class_list().remove_1("text-[#166534]");
}

/// A labeled, read-only, copy-to-clipboard input row. Replaces the
/// `copy_row()` string helper (formerly duplicated per page) plus the global
/// `copyToClipboard` `<script>` + inline `onclick="…"` it relied on.
#[component]
pub fn CopyRow(label: &'static str, value: String) -> impl IntoView {
    let icon_ref: NodeRef<html::Span> = NodeRef::new();
    #[cfg(feature = "hydrate")]
    let value_for_click = value.clone();
    let on_click = move |_| {
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::JsCast;
            if let Some(icon) = icon_ref.get() {
                wasm_bindgen_futures::spawn_local(copy_to_clipboard_and_flash_icon(
                    value_for_click.clone(),
                    icon.unchecked_into(),
                ));
            }
        }
    };
    view! {
        <div class="space-y-sm mt-sm">
            <label class="font-label-md text-label-md text-on-surface-variant block uppercase tracking-wide">{label}</label>
            <div class="flex gap-sm">
                <input
                    class="w-full h-10 px-md bg-surface-container-low border border-outline-variant font-code text-code focus:outline-none focus:ring-0 cursor-default"
                    readonly=true
                    type="text"
                    value=value
                />
                <button
                    type="button"
                    class="w-10 h-10 border border-outline-variant flex items-center justify-center hover:bg-surface-container-high transition-all active:bg-surface-container-highest shrink-0"
                    on:click=on_click
                >
                    <span node_ref=icon_ref class="material-symbols-outlined">"content_copy"</span>
                </button>
            </div>
        </div>
    }
}
