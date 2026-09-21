pub(crate) const GOOGLE_FONTS_HREF: &str = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&icon_names=add,arrow_back,arrow_forward,calendar_add_on,calendar_month,calendar_today,check_circle,close,content_copy,database,error,event_available,link,login,logout,open_in_new,security,sync,sync_alt,verified,warning&display=swap";

pub(crate) const ONBOARDING_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
.custom-checkbox:checked { background-color: #000000; border-color: #000000; }
.card-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
"#;

pub(crate) fn live_reload_script() -> &'static str {
    if cfg!(debug_assertions) {
        r#"<script>(function(){function c(){var s=new WebSocket('ws://'+location.hostname+':3002/live_reload');s.onmessage=function(){location.reload();};s.onclose=function(){setTimeout(c,1000);};}c();})();</script>"#
    } else {
        ""
    }
}

fn cookie_lang(cookie_header: &str) -> Option<&'static str> {
    for pair in cookie_header.split(';') {
        if let Some((k, v)) = pair.trim().split_once('=') {
            if k.trim() == "lang" {
                if v == "en" {
                    return Some("en");
                }
                if v == "vi" {
                    return Some("vi");
                }
            }
        }
    }
    None
}

pub(crate) fn detect_lang() -> &'static str {
    #[cfg(feature = "ssr")]
    {
        let Some(parts) = leptos::prelude::use_context::<axum::http::request::Parts>() else {
            return "vi";
        };
        if let Some(lang) = parts
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(cookie_lang)
        {
            return lang;
        }
        let accept_language = parts
            .headers
            .get("accept-language")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let most_preferred = accept_language
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let lang = if most_preferred.starts_with("en") {
            "en"
        } else {
            "vi"
        };
        // No explicit "lang" cookie was sent — write one back reflecting the
        // Accept-Language-derived choice, so the client's post-hydration
        // render (which can only read document.cookie, not the original
        // request headers) resolves the same language instead of flashing
        // to a hardcoded default.
        if let Some(res_options) = leptos::prelude::use_context::<leptos_axum::ResponseOptions>()
        {
            res_options.append_header(
                axum::http::header::SET_COOKIE,
                axum::http::HeaderValue::from_str(&format!("lang={lang}; Path=/; Max-Age=31536000"))
                    .expect("lang cookie value is always a valid header value"),
            );
        }
        return lang;
    }
    #[cfg(not(feature = "ssr"))]
    {
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            use wasm_bindgen::JsCast;
            if let Ok(html_document) = document.dyn_into::<web_sys::HtmlDocument>() {
                if let Ok(cookie) = html_document.cookie() {
                    if let Some(lang) = cookie_lang(&cookie) {
                        return lang;
                    }
                }
            }
        }
        "vi"
    }
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

fn lang_toggle_html(lang: &str, current_path: &str) -> String {
    let (other_code, other_label) = if lang == "en" {
        ("vi", "VI")
    } else {
        ("en", "EN")
    };
    let escaped_path = current_path.replace('&', "%26");
    format!(
        r#"<a href="/lang/{other_code}?next={escaped_path}" class="text-label-md text-on-surface-variant hover:text-primary transition-colors">{other_label}</a>"#
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn top_nav_html(email: &str, lang: &str, current_path: &str) -> String {
    let logout_title = if lang == "en" { "Log out" } else { "Đăng xuất" };
    format!(
        r#"<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
<div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
<a href="/" class="text-h1 font-semibold tracking-tighter text-primary hover:opacity-70 transition-opacity">NotionCal</a>
<div class="flex items-center space-x-md">
{lang_toggle}
<span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
<a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="{logout_title}">
<span class="material-symbols-outlined">logout</span>
</a>
</div>
</div>
</header>"#,
        lang_toggle = lang_toggle_html(lang, current_path),
        email = html_escape(email),
    )
}
