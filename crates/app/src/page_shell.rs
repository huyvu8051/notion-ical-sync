pub(crate) const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

pub(crate) const ROOT_ICON_STYLE: &str =
    ".material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }";

pub(crate) const HOME_HEADER_STYLE: &str =
    ".glass-header { backdrop-filter: blur(8px); background: rgba(251, 249, 249, 0.85); }";

pub(crate) const HOME_HEADER_HTML: &str = r##"<header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
<div class="flex items-center gap-8">
<a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
<span class="material-symbols-outlined text-primary">calendar_month</span>
NotionCal
</a>
<nav class="hidden md:flex items-center gap-6">
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#how-it-works">How it works</a>
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="/#pricing">Pricing</a>
</nav>
</div>
<div class="flex items-center gap-4">
<a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me" rel="external">Log in / Sign up</a>
</div>
</div>
</header>"##;

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
