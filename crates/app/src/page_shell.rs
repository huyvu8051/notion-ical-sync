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

/// The shared site footer — same look everywhere (originally the landing
/// page's footer), now with the client-local timezone label restored
/// alongside the NotionCal wordmark.
#[component]
pub fn PageFooter() -> impl IntoView {
    view! {
        <footer class="border-t border-outline-variant mt-xl">
            <div class="w-full py-lg flex flex-col md:flex-row justify-between items-center gap-md text-center md:text-left">
                <div class="flex flex-col md:flex-row items-center gap-md">
                    <span class="text-h3 font-bold text-primary">"NotionCal"</span>
                    <ClientTimezone class="text-label-md text-on-surface-variant".to_string()/>
                </div>
                <div class="flex items-center gap-6">
                    <a class="text-label-md text-on-surface-variant hover:text-primary transition-colors" href="/privacy">"Privacy Policy"</a>
                    <a class="text-label-md text-on-surface-variant hover:text-primary transition-colors" href="/terms">"Terms of Service"</a>
                </div>
            </div>
        </footer>
    }
}

/// The shared site header. Logged out (`email` is `None`): marketing nav
/// links + "Log in / Sign up", as on the landing page. Logged in
/// (`email` is `Some`): nav links hidden, replaced by the user's email +
/// a logout button — same header everywhere, just the identity slot swaps.
#[component]
pub fn HomeHeader(#[prop(optional)] email: Option<String>) -> impl IntoView {
    let is_authed = email.is_some();
    view! {
        <header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
                <div class="flex items-center gap-8">
                    <a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
                        <span class="material-symbols-outlined text-primary !text-[24px]">"calendar_month"</span>
                        "NotionCal"
                    </a>
                    {(!is_authed).then(|| view! {
                        <nav class="hidden md:flex items-center gap-6">
                            <a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#how-it-works">"How it works"</a>
                            <a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#pricing">"Pricing"</a>
                        </nav>
                    })}
                </div>
                <div class="flex items-center gap-4">
                    {match email {
                        Some(email) => view! {
                            <div class="flex items-center space-x-md">
                                <span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
                                <a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="Log out">
                                    <span class="material-symbols-outlined !text-[20px]">"logout"</span>
                                </a>
                            </div>
                        }.into_any(),
                        None => view! {
                            <a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me" rel="external">"Log in / Sign up"</a>
                        }.into_any(),
                    }}
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
    icon.set_text_content(Some("check_circle"));
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
