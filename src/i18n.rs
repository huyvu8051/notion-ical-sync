//! Minimal i18n: language is detected from a `lang` cookie (explicit user
//! choice, set via `/lang/{code}`) falling back to the browser's
//! `Accept-Language` header, falling back to Vietnamese (the app's original
//! and still-primary audience). No translation-string crate — pages that
//! need it hold their own EN/VI copy (either two full HTML consts for fully
//! static pages, or a small per-page `Labels` struct for pages with dynamic
//! content interleaved).

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

    /// Crude but sufficient: Accept-Language lists tags in preference order
    /// like "en-US,en;q=0.9,vi;q=0.8" — the first tag decides. Vietnamese by
    /// default since that's the app's primary audience and every existing
    /// page was written in Vietnamese first.
    fn from_accept_language(header_val: &str) -> Self {
        let first_tag = header_val.split(',').next().unwrap_or("").trim().to_lowercase();
        if first_tag.starts_with("en") {
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
            .map(Self::from_accept_language)
            .unwrap_or(Lang::Vi)
    }
}

impl<S: Sync> FromRequestParts<S> for Lang {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::detect(&parts.headers))
    }
}

/// `/lang/{en|vi}?next=/some/path` — sets the override cookie (one year) and
/// redirects back. `next` must be a same-app relative path; anything else
/// (missing, or not starting with `/`) falls back to `/` rather than acting
/// as an open redirect.
pub async fn set_lang(Path(code): Path<String>, Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let lang = if code == "en" { Lang::En } else { Lang::Vi };
    let next = params
        .get("next")
        .filter(|n| n.starts_with('/') && !n.starts_with("//"))
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let cookie = format!("lang={}; Path=/; Max-Age=31536000; SameSite=Lax", lang.code());
    ([(header::SET_COOKIE, cookie)], Redirect::to(&next))
}

/// Small `EN | VI` toggle linking to whichever language isn't currently
/// active, back to `current_path` once set.
pub fn lang_toggle(current: Lang, current_path: &str) -> String {
    let other = current.other();
    let path = current_path.replace('&', "%26");
    format!(
        r#"<a href="/lang/{code}?next={path}" class="text-label-md text-on-surface-variant hover:text-primary transition-colors">{label}</a>"#,
        code = other.code(),
        label = match other {
            Lang::En => "EN",
            Lang::Vi => "VI",
        },
    )
}
