use axum::extract::{FromRequestParts, Path, Query};
use axum::http::{header, request::Parts, HeaderMap};
use axum::response::{IntoResponse, Redirect};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Vi,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Vi => "vi",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Lang::En => Lang::Vi,
            Lang::Vi => Lang::En,
        }
    }

    pub fn from_code(code: &str) -> Self {
        if code == "en" {
            Lang::En
        } else {
            Lang::Vi
        }
    }

    fn from_cookie_header(cookie_header: &str) -> Option<Self> {
        cookie_header.split(';').find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            if key != "lang" {
                return None;
            }
            match value {
                "en" => Some(Lang::En),
                "vi" => Some(Lang::Vi),
                _ => None,
            }
        })
    }

    fn from_accept_language_header(header_value: &str) -> Self {
        let most_preferred_tag = header_value
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if most_preferred_tag.starts_with("en") {
            Lang::En
        } else {
            Lang::Vi
        }
    }

    pub fn detect(headers: &HeaderMap) -> Self {
        if let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
            if let Some(lang) = Self::from_cookie_header(cookie) {
                return lang;
            }
        }
        headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .map(Self::from_accept_language_header)
            .unwrap_or(Lang::Vi)
    }
}

impl<S: Sync> FromRequestParts<S> for Lang {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::detect(&parts.headers))
    }
}

fn is_safe_same_app_redirect_path(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//")
}

pub async fn set_lang(
    Path(code): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let lang = if code == "en" { Lang::En } else { Lang::Vi };
    let next = params
        .get("next")
        .filter(|n| is_safe_same_app_redirect_path(n))
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let cookie = format!(
        "lang={}; Path=/; Max-Age=31536000; SameSite=Lax",
        lang.code()
    );
    ([(header::SET_COOKIE, cookie)], Redirect::to(&next))
}

pub fn top_nav_html(email: &str, lang: Lang, current_path: &str) -> String {
    let logout_title = match lang {
        Lang::Vi => "Đăng xuất",
        Lang::En => "Log out",
    };
    format!(
        r#"<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
<div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
<a href="/" class="text-h1 font-semibold tracking-tighter text-primary hover:opacity-70 transition-opacity">NotionCal</a>
<div class="flex items-center space-x-md">
{lang_toggle}
<span id="client-tz" class="text-label-md text-on-surface-variant"></span>
<span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
<a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="{logout_title}">
<span class="material-symbols-outlined">logout</span>
</a>
</div>
</div>
</header>"#,
        lang_toggle = lang_toggle(lang, current_path),
        email = crate::session::html_escape(email),
    )
}

pub fn lang_toggle(current: Lang, current_path: &str) -> String {
    let other = current.other();
    let escaped_path = current_path.replace('&', "%26");
    format!(
        r#"<a href="/lang/{code}?next={escaped_path}" class="text-label-md text-on-surface-variant hover:text-primary transition-colors">{label}</a>"#,
        code = other.code(),
        label = match other {
            Lang::En => "EN",
            Lang::Vi => "VI",
        },
    )
}
