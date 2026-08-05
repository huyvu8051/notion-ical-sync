//! Notion OAuth: this app's permission to read/write a specific user's own
//! Notion workspace — a "Public Integration" grant, separate from the SaaS's
//! own login in auth.rs (Keycloak). A user authenticates via Keycloak first,
//! then goes through this flow to grant Notion access and pick which
//! databases become calendars.

use std::collections::HashMap;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use base64::Engine;
use rand::Rng;
use serde::Deserialize;
use tracing::error;

use crate::auth::{find_or_create_user, html_escape, AUTH_STYLE};
use crate::AppState;

const NOTION_VERSION: &str = "2025-09-03";

/// Config for the Notion Public Integration OAuth flow. Absent (`None`) in
/// AppState until `NOTION_OAUTH_CLIENT_ID`/`NOTION_OAUTH_CLIENT_SECRET` are
/// set — connect routes render a "not configured" page instead of panicking,
/// same posture as `webhook_secret` being optional.
#[derive(Debug, Clone)]
pub struct NotionOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl NotionOAuthConfig {
    pub fn from_env(app_base_url: &str) -> Option<Self> {
        let client_id = std::env::var("NOTION_OAUTH_CLIENT_ID").ok()?;
        let client_secret = std::env::var("NOTION_OAUTH_CLIENT_SECRET").ok()?;
        Some(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{app_base_url}/oauth/notion/callback"),
        })
    }
}

fn generate_token(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Encrypts a CalDAV password for storage in `caldav_password_encrypted`, so
/// the dashboard's "Hiện mật khẩu" action can recover it later — separate
/// from `caldav_password_hash` (Argon2, one-way), which is what actual
/// CalDAV Basic Auth verifies against and is unaffected by this.
fn encrypt_password(key: &[u8; 32], plaintext: &str) -> Option<String> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).ok()?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Some(base64::engine::general_purpose::STANDARD.encode(combined))
}

fn decrypt_password(key: &[u8; 32], encoded: &str) -> Option<String> {
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if combined.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

/// Every failure state shown via `error_page` below, across the Notion
/// connect flow and billing checkout — kept as one enum (rather than each
/// call site building its own string) so the VI/EN pair for each case lives
/// in exactly one place instead of getting hardcoded ad hoc at 29 call sites.
pub(crate) enum OauthError {
    NotionNotConfigured,
    TryAgain,
    NotionDenied(String),
    MissingAuthCode,
    InvalidSession,
    CantReachNotion,
    NotionRejectedToken,
    InvalidNotionResponse,
    NotionResponseMissingFields,
    Generic,
    FailedToSaveConnection,
    ConnectionNotFound,
    FailedToListDatabases,
    InvalidRequest,
    CalendarNotFound,
    FailedToDeleteCalendar,
    RevealPasswordNotConfigured,
    PasswordPredatesReveal,
    FailedToRegeneratePassword,
    BillingNotConfigured,
    FailedToCreateCheckoutSession,
}

impl OauthError {
    fn message(&self, lang: crate::i18n::Lang) -> String {
        use crate::i18n::Lang;
        match (self, lang) {
            (Self::NotionNotConfigured, Lang::Vi) => "Notion OAuth chưa được cấu hình trên server này.".to_string(),
            (Self::NotionNotConfigured, Lang::En) => "Notion OAuth isn't configured on this server.".to_string(),
            (Self::TryAgain, Lang::Vi) => "Có lỗi xảy ra, vui lòng thử lại.".to_string(),
            (Self::TryAgain, Lang::En) => "Something went wrong, please try again.".to_string(),
            (Self::NotionDenied(reason), Lang::Vi) => format!("Notion từ chối cấp quyền: {reason}"),
            (Self::NotionDenied(reason), Lang::En) => format!("Notion denied the authorization request: {reason}"),
            (Self::MissingAuthCode, Lang::Vi) => "Thiếu mã xác thực từ Notion.".to_string(),
            (Self::MissingAuthCode, Lang::En) => "Missing authorization code from Notion.".to_string(),
            (Self::InvalidSession, Lang::Vi) => "Phiên xác thực không hợp lệ, vui lòng thử lại.".to_string(),
            (Self::InvalidSession, Lang::En) => "Invalid auth session, please try again.".to_string(),
            (Self::CantReachNotion, Lang::Vi) => "Không thể kết nối tới Notion.".to_string(),
            (Self::CantReachNotion, Lang::En) => "Couldn't connect to Notion.".to_string(),
            (Self::NotionRejectedToken, Lang::Vi) => "Notion từ chối yêu cầu trao đổi token.".to_string(),
            (Self::NotionRejectedToken, Lang::En) => "Notion rejected the token exchange request.".to_string(),
            (Self::InvalidNotionResponse, Lang::Vi) => "Phản hồi từ Notion không hợp lệ.".to_string(),
            (Self::InvalidNotionResponse, Lang::En) => "Invalid response from Notion.".to_string(),
            (Self::NotionResponseMissingFields, Lang::Vi) => "Phản hồi từ Notion thiếu access_token hoặc workspace_id.".to_string(),
            (Self::NotionResponseMissingFields, Lang::En) => "Notion's response is missing access_token or workspace_id.".to_string(),
            (Self::Generic, Lang::Vi) => "Có lỗi xảy ra.".to_string(),
            (Self::Generic, Lang::En) => "Something went wrong.".to_string(),
            (Self::FailedToSaveConnection, Lang::Vi) => "Không thể lưu kết nối Notion.".to_string(),
            (Self::FailedToSaveConnection, Lang::En) => "Failed to save the Notion connection.".to_string(),
            (Self::ConnectionNotFound, Lang::Vi) => "Không tìm thấy kết nối Notion này.".to_string(),
            (Self::ConnectionNotFound, Lang::En) => "This Notion connection wasn't found.".to_string(),
            (Self::FailedToListDatabases, Lang::Vi) => "Không thể lấy danh sách cơ sở dữ liệu từ Notion.".to_string(),
            (Self::FailedToListDatabases, Lang::En) => "Failed to list databases from Notion.".to_string(),
            (Self::InvalidRequest, Lang::Vi) => "Yêu cầu không hợp lệ.".to_string(),
            (Self::InvalidRequest, Lang::En) => "Invalid request.".to_string(),
            (Self::CalendarNotFound, Lang::Vi) => "Không tìm thấy calendar này.".to_string(),
            (Self::CalendarNotFound, Lang::En) => "This calendar wasn't found.".to_string(),
            (Self::FailedToDeleteCalendar, Lang::Vi) => "Không thể xoá calendar này.".to_string(),
            (Self::FailedToDeleteCalendar, Lang::En) => "Failed to delete this calendar.".to_string(),
            (Self::RevealPasswordNotConfigured, Lang::Vi) => {
                "Tính năng hiện mật khẩu chưa được cấu hình trên server này — dùng \"Tạo lại mật khẩu\" thay thế.".to_string()
            }
            (Self::RevealPasswordNotConfigured, Lang::En) => {
                "The reveal-password feature isn't configured on this server — use \"Regenerate password\" instead.".to_string()
            }
            (Self::PasswordPredatesReveal, Lang::Vi) => {
                "Mật khẩu này được tạo trước khi tính năng \"Hiện lại\" ra mắt nên không thể khôi phục — hãy dùng \"Tạo lại mật khẩu\".".to_string()
            }
            (Self::PasswordPredatesReveal, Lang::En) => {
                "This password was created before the \"reveal\" feature shipped, so it can't be recovered — use \"Regenerate password\" instead.".to_string()
            }
            (Self::FailedToRegeneratePassword, Lang::Vi) => "Không thể tạo lại mật khẩu.".to_string(),
            (Self::FailedToRegeneratePassword, Lang::En) => "Failed to regenerate the password.".to_string(),
            (Self::BillingNotConfigured, Lang::Vi) => "Tính năng thanh toán chưa được cấu hình trên server này.".to_string(),
            (Self::BillingNotConfigured, Lang::En) => "Billing isn't configured on this server.".to_string(),
            (Self::FailedToCreateCheckoutSession, Lang::Vi) => "Không thể tạo phiên thanh toán.".to_string(),
            (Self::FailedToCreateCheckoutSession, Lang::En) => "Failed to create a checkout session.".to_string(),
        }
    }
}

pub(crate) fn error_page(lang: crate::i18n::Lang, err: OauthError) -> axum::response::Response {
    let (html_lang, back_label) = match lang {
        crate::i18n::Lang::Vi => ("vi", "Quay lại"),
        crate::i18n::Lang::En => ("en", "Back"),
    };
    Html(format!(
        r#"<!doctype html>
<html lang="{html_lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="logout" href="/me">{back_label}</a></div>
<p class="hint">{}</p>
</body></html>"#,
        html_escape(&err.message(lang))
    ))
    .into_response()
}

/// Shared Tailwind head for both onboarding screens — ports the exact
/// design-token config from the Stitch "Kết nối Notion" / "Chọn cơ sở dữ
/// liệu" mockups (project 7966553897766226544).
const ONBOARDING_HEAD: &str = r##"
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet">
<script id="tailwind-config">
tailwind.config = {
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "error": "#ba1a1a", "error-container": "#ffdad6", "on-error-container": "#93000a",
        "surface-container-low": "#f5f3f3", "surface-container-high": "#e9e8e7", "surface-container-highest": "#e3e2e2",
        "surface-container": "#efeded", "surface-container-lowest": "#ffffff",
        "on-surface-variant": "#444748", "secondary": "#0058be"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "xs": "4px", "margin-desktop": "32px", "margin-mobile": "16px", "xl": "40px", "gutter": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h3": ["16px", { lineHeight: "1.5", letterSpacing: "-0.01em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  }
}
</script>
<style>
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
.custom-checkbox:checked { background-color: #000000; border-color: #000000; }
.card-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
</style>
"##;

fn onboarding_top_nav(email: &str, lang: crate::i18n::Lang) -> String {
    let logout = match lang {
        crate::i18n::Lang::Vi => "Đăng xuất",
        crate::i18n::Lang::En => "Log out",
    };
    format!(
        r#"<header class="bg-surface border-b border-outline-variant fixed top-0 left-0 right-0 z-50">
<nav class="flex justify-between items-center w-full px-margin-desktop h-[56px] max-w-[1280px] mx-auto">
<span class="text-h2 font-semibold text-primary">NotionCal</span>
<div class="flex items-center gap-lg">
<span class="text-on-surface-variant text-label-md">{}</span>
<a class="text-primary hover:bg-surface-container-low transition-colors px-sm py-xs rounded-lg text-label-md" href="/logout">{logout}</a>
</div>
</nav>
</header>"#,
        html_escape(email)
    )
}

struct ConnectNotionLabels {
    title: &'static str,
    heading: &'static str,
    body: &'static str,
    connect_cta: &'static str,
    bullet_read_write: &'static str,
    bullet_disconnect: &'static str,
    bullet_no_sharing: &'static str,
    privacy_link: &'static str,
    terms_link: &'static str,
}

const CONNECT_NOTION_LABELS_VI: ConnectNotionLabels = ConnectNotionLabels {
    title: "Kết nối Notion",
    heading: "Kết nối không gian làm việc Notion của bạn",
    body: "Chúng tôi cần quyền truy cập vào không gian làm việc Notion của bạn để tìm và đồng bộ hóa các cơ sở dữ liệu bạn chọn. Bạn sẽ chọn chính xác trang nào cần chia sẻ ở bước tiếp theo trên Notion.",
    connect_cta: "Kết nối với Notion",
    bullet_read_write: "Chỉ đọc và ghi vào các trang bạn cho phép",
    bullet_disconnect: "Có thể ngắt kết nối bất cứ lúc nào",
    bullet_no_sharing: "Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba",
    privacy_link: "Chính sách bảo mật",
    terms_link: "Điều khoản dịch vụ",
};

const CONNECT_NOTION_LABELS_EN: ConnectNotionLabels = ConnectNotionLabels {
    title: "Connect Notion",
    heading: "Connect your Notion workspace",
    body: "We need access to your Notion workspace to find and sync the databases you choose. You'll pick exactly which pages to share in the next step, on Notion.",
    connect_cta: "Connect to Notion",
    bullet_read_write: "Only reads and writes the pages you allow",
    bullet_disconnect: "Disconnect anytime",
    bullet_no_sharing: "Never shares your data with third parties",
    privacy_link: "Privacy Policy",
    terms_link: "Terms of Service",
};

/// Step 1 confirmation screen (matches the Stitch "Kết nối Notion" design) —
/// shown before we actually redirect away to Notion's consent screen.
pub async fn connect_notion_page(
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
) -> impl IntoResponse {
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let l = match lang {
        crate::i18n::Lang::Vi => &CONNECT_NOTION_LABELS_VI,
        crate::i18n::Lang::En => &CONNECT_NOTION_LABELS_EN,
    };
    let html_lang = lang.code();
    Html(format!(
        r#"<!doctype html>
<html lang="{html_lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — NotionCal</title>{ONBOARDING_HEAD}</head>
<body class="min-h-screen flex flex-col">
{top_nav}
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
</main>
</body></html>"#,
        title = l.title,
        top_nav = onboarding_top_nav(email, lang),
        heading = l.heading,
        body = l.body,
        connect_cta = l.connect_cta,
        bullet_read_write = l.bullet_read_write,
        bullet_disconnect = l.bullet_disconnect,
        bullet_no_sharing = l.bullet_no_sharing,
        privacy_link = l.privacy_link,
        terms_link = l.terms_link,
    ))
}

/// Actually redirects to Notion's OAuth consent screen, stashing a CSRF
/// state token in the session first.
pub async fn connect_notion_start(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
) -> impl IntoResponse {
    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(lang, OauthError::NotionNotConfigured);
    };

    let oauth_state = generate_token(32);
    if let Err(e) = session.insert("notion_oauth_state", &oauth_state).await {
        error!("failed to store notion oauth state: {}", e);
        return error_page(lang, OauthError::TryAgain);
    }

    let mut url = url::Url::parse("https://api.notion.com/v1/oauth/authorize").expect("static url");
    url.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("owner", "user")
        .append_pair("redirect_uri", &cfg.redirect_uri)
        .append_pair("state", &oauth_state);

    Redirect::to(url.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Notion redirects here after the user grants (or denies) access. Exchanges
/// the code for an access token and upserts `notion_connections`.
pub async fn notion_oauth_callback(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    if let Some(err) = params.error {
        return error_page(lang, OauthError::NotionDenied(err));
    }
    let Some(code) = params.code else {
        return error_page(lang, OauthError::MissingAuthCode);
    };

    let expected_state: Option<String> = session.get("notion_oauth_state").await.unwrap_or(None);
    let _ = session.remove::<String>("notion_oauth_state").await;
    if expected_state.is_none() || expected_state.as_deref() != params.state.as_deref() {
        return error_page(lang, OauthError::InvalidSession);
    }

    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(lang, OauthError::NotionNotConfigured);
    };

    let resp = match state
        .client
        .post("https://api.notion.com/v1/oauth/token")
        .basic_auth(&cfg.client_id, Some(&cfg.client_secret))
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": cfg.redirect_uri,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("notion token exchange request failed: {}", e);
            return error_page(lang, OauthError::CantReachNotion);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        error!("notion token exchange failed {}: {}", status, txt);
        return error_page(lang, OauthError::NotionRejectedToken);
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to parse notion token response: {}", e);
            return error_page(lang, OauthError::InvalidNotionResponse);
        }
    };

    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_id = body
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_name = body
        .get("workspace_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let bot_id = body
        .get("bot_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if access_token.is_empty() || workspace_id.is_empty() {
        return error_page(lang, OauthError::NotionResponseMissingFields);
    }

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(e) => {
            error!("failed to find_or_create_user: {}", e);
            return error_page(lang, OauthError::Generic);
        }
    };

    let connection_id: i64 = match sqlx::query_scalar(
        "INSERT INTO notion_connections (user_id, notion_access_token, workspace_id, workspace_name, bot_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, workspace_id) DO UPDATE SET
             notion_access_token = EXCLUDED.notion_access_token,
             workspace_name = EXCLUDED.workspace_name,
             bot_id = EXCLUDED.bot_id
         RETURNING id",
    )
    .bind(user_id)
    .bind(&access_token)
    .bind(&workspace_id)
    .bind(&workspace_name)
    .bind(&bot_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!("failed to upsert notion_connection: {}", e);
            return error_page(lang, OauthError::FailedToSaveConnection);
        }
    };

    Redirect::to(&format!(
        "/connect/notion/databases?connection_id={connection_id}"
    ))
    .into_response()
}

struct DatabaseCandidate {
    database_id: String,
    data_source_id: String,
    title: String,
    icon_emoji: Option<String>,
    /// Name of the first date-typed property found, if any. Databases
    /// without one can't be synced (refresh_db needs a date property to
    /// filter/sort on).
    date_property: Option<String>,
}

/// Searches the workspace for every data source (Notion's 2025-09-03 API
/// split "database" into a container plus one-or-more data sources —
/// `properties` now lives on the data source, not the database, so search
/// for `data_source` objects directly rather than `database` objects) the
/// user granted access to, checking each one for a date property (needed
/// for both sync and this picker's compatibility check).
async fn list_syncable_databases(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<DatabaseCandidate>, String> {
    let resp = client
        .post("https://api.notion.com/v1/search")
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({ "filter": { "value": "data_source", "property": "object" } }))
        .send()
        .await
        .map_err(|e| format!("search request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Notion search error {status}: {txt}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;
    let results = body
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    let mut candidates = Vec::new();
    for ds in results {
        let Some(data_source_id) = ds.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(database_id) = ds
            .get("parent")
            .and_then(|p| p.get("database_id"))
            .and_then(|id| id.as_str())
        else {
            continue; // parent isn't a database (shouldn't happen for object=data_source, but be defensive)
        };

        let title = ds
            .get("title")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("plain_text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)")
            .to_string();
        let icon_emoji = ds
            .get("icon")
            .and_then(|icon| icon.get("emoji"))
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let date_property = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .and_then(|props| {
                props
                    .iter()
                    .find(|(_, def)| def.get("type").and_then(|t| t.as_str()) == Some("date"))
                    .map(|(name, _)| name.clone())
            });

        candidates.push(DatabaseCandidate {
            database_id: database_id.to_string(),
            data_source_id: data_source_id.to_string(),
            title,
            icon_emoji,
            date_property,
        });
    }

    Ok(candidates)
}

async fn connection_token_for_user(
    state: &AppState,
    connection_id: i64,
    user_id: i64,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT notion_access_token FROM notion_connections WHERE id = $1 AND user_id = $2",
    )
    .bind(connection_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None)
}

#[derive(Debug, Deserialize)]
pub struct DatabasesPageParams {
    connection_id: i64,
}

/// Step 2 (matches the Stitch "Pick a database" design) — lists every
/// database found in the just-connected workspace, disabling ones without a
/// date property.
pub async fn pick_databases_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Query(params): Query<DatabasesPageParams>,
) -> impl IntoResponse {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return error_page(lang, OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, params.connection_id, user_id).await
    else {
        return error_page(lang, OauthError::ConnectionNotFound);
    };

    let candidates = match list_syncable_databases(&state.client, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page(lang, OauthError::FailedToListDatabases);
        }
    };

    if candidates.is_empty() {
        return Html(format!(
            r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{ONBOARDING_HEAD}</head>
<body class="min-h-screen flex flex-col">
{top_nav}
<main class="flex-grow flex flex-col items-center justify-center pt-[56px] px-margin-mobile text-center">
<h1 class="text-h1 text-primary mb-sm">Chọn cơ sở dữ liệu để đồng bộ</h1>
<p class="text-on-surface-variant text-body-lg">Không tìm thấy cơ sở dữ liệu nào bạn đã cấp quyền. <a class="text-secondary underline" href="/connect/notion/start">Cấp thêm quyền truy cập trên Notion</a>.</p>
</main>
</body></html>"#,
            top_nav = onboarding_top_nav(&email, lang),
        ))
        .into_response();
    }

    let rows: String = candidates
        .iter()
        .map(|c| {
            let icon = c.icon_emoji.clone().unwrap_or_else(|| "📄".to_string());
            match &c.date_property {
                Some(date_prop) => format!(
                    r#"<label class="group flex items-center gap-md p-md bg-white border border-outline-variant rounded-lg cursor-pointer hover:border-primary transition-all duration-200 card-shadow">
<input type="checkbox" name="db_ids" value="{db_id}" checked class="w-5 h-5 border-2 border-outline-variant rounded-sm text-primary focus:ring-0 focus:ring-offset-0 custom-checkbox">
<div class="flex items-center justify-center w-10 h-10 bg-surface-container rounded-lg text-xl">{icon}</div>
<div class="flex-grow">
<h3 class="text-h3 text-primary">{title}</h3>
<p class="text-on-surface-variant text-label-md flex items-center gap-xs">
<span class="material-symbols-outlined text-[14px]">calendar_today</span>
Có thuộc tính ngày: {date_prop}
</p>
</div>
</label>"#,
                    db_id = html_escape(&c.database_id),
                    icon = html_escape(&icon),
                    title = html_escape(&c.title),
                    date_prop = html_escape(date_prop),
                ),
                None => format!(
                    r#"<div class="flex items-center gap-md p-md bg-surface-container-low border border-outline-variant opacity-60 rounded-lg grayscale cursor-not-allowed">
<input type="checkbox" disabled class="w-5 h-5 border-2 border-outline-variant rounded-sm bg-surface-container-highest cursor-not-allowed">
<div class="flex items-center justify-center w-10 h-10 bg-surface-container-high rounded-lg text-xl">{icon}</div>
<div class="flex-grow">
<div class="flex items-center gap-sm">
<h3 class="text-h3 text-on-surface-variant">{title}</h3>
<span class="bg-error-container text-on-error-container text-[10px] px-xs py-[2px] rounded font-bold uppercase tracking-wider">Lỗi</span>
</div>
<p class="text-error text-label-md flex items-center gap-xs mt-1">
<span class="material-symbols-outlined text-[14px]">warning</span>
Không tìm thấy thuộc tính ngày
</p>
</div>
</div>"#,
                    icon = html_escape(&icon),
                    title = html_escape(&c.title),
                ),
            }
        })
        .collect();

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Chọn cơ sở dữ liệu — NotionCal</title>{ONBOARDING_HEAD}</head>
<body class="min-h-screen flex flex-col">
{top_nav}
<main class="flex-grow flex flex-col pt-[80px] pb-32">
<div class="max-w-[720px] mx-auto w-full px-margin-mobile md:px-0">
<section class="mb-xl">
<h1 class="text-h1 text-primary mb-sm">Chọn cơ sở dữ liệu để đồng bộ</h1>
<p class="text-on-surface-variant text-body-lg">Chúng tôi đã tìm thấy các cơ sở dữ liệu sau trong không gian làm việc Notion của bạn. Chọn (các) cơ sở dữ liệu bạn muốn biến thành lịch.</p>
</section>
<form method="post" action="/connect/notion/databases">
<input type="hidden" name="connection_id" value="{connection_id}">
<div class="space-y-md">{rows}</div>
<div class="mt-xl text-center">
<a class="text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/connect/notion/start">Không thấy cơ sở dữ liệu bạn cần? Cấp thêm quyền truy cập trên Notion</a>
</div>
<div class="fixed bottom-0 left-0 right-0 bg-white/80 backdrop-blur-md border-t border-outline-variant py-md z-40">
<div class="max-w-[1280px] mx-auto px-margin-desktop flex justify-between items-center">
<a class="px-lg h-[40px] border border-outline-variant text-primary text-label-md rounded hover:bg-surface-container-low transition-colors flex items-center gap-sm" href="/me">
<span class="material-symbols-outlined text-[18px]">arrow_back</span>
Quay lại
</a>
<button type="submit" id="continue-btn" class="px-lg h-[40px] bg-primary text-white text-label-md rounded hover:opacity-90 transition-all flex items-center gap-sm active:scale-95 shadow-sm">
Tiếp tục
<span class="material-symbols-outlined text-[18px]">arrow_forward</span>
</button>
</div>
</div>
</form>
</div>
</main>
<script>
document.addEventListener('change', function () {{
  var n = document.querySelectorAll('input[name="db_ids"]:checked').length;
  var btn = document.getElementById('continue-btn');
  btn.innerHTML = n > 0
    ? 'Tiếp tục với ' + n + ' cơ sở dữ liệu <span class="material-symbols-outlined text-[18px]">arrow_forward</span>'
    : 'Chọn ít nhất 1 cơ sở dữ liệu <span class="material-symbols-outlined text-[18px]">error</span>';
}});
</script>
</body></html>"#,
        top_nav = onboarding_top_nav(&email, lang),
        connection_id = params.connection_id,
    ))
    .into_response()
}

struct CreateCalendarsForm {
    connection_id: i64,
    db_ids: Vec<String>,
}

impl CreateCalendarsForm {
    /// Parsed by hand from the raw body via `url::form_urlencoded` rather
    /// than `axum::Form` — `serde_urlencoded` (what `Form` uses) can't
    /// deserialize a `Vec<String>` field from a single `db_ids=x` pair (only
    /// happens to work when 2+ checkboxes are checked), which broke the
    /// single-database-selected case, the most common one.
    fn parse(body: &str) -> Option<Self> {
        let mut connection_id = None;
        let mut db_ids = Vec::new();
        for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
            match key.as_ref() {
                "connection_id" => connection_id = value.parse::<i64>().ok(),
                "db_ids" => db_ids.push(value.into_owned()),
                _ => {}
            }
        }
        Some(Self {
            connection_id: connection_id?,
            db_ids,
        })
    }
}

/// Creates one `calendars` row (with freshly generated CalDAV credentials)
/// per selected, syncable database. Re-fetches the candidate list from
/// Notion server-side rather than trusting metadata from the form, so a
/// tampered request can at most select an id that isn't actually syncable
/// (silently skipped) — never inject arbitrary data_source_id/date_property.
pub async fn create_calendars(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let body = String::from_utf8_lossy(&body);
    let Some(form) = CreateCalendarsForm::parse(&body) else {
        return error_page(lang, OauthError::InvalidRequest);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return error_page(lang, OauthError::Generic),
    };

    let Some(access_token) = connection_token_for_user(&state, form.connection_id, user_id).await
    else {
        return error_page(lang, OauthError::ConnectionNotFound);
    };

    if form.db_ids.is_empty() {
        return Redirect::to("/me").into_response();
    }

    let candidates = match list_syncable_databases(&state.client, &access_token).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to list notion databases: {}", e);
            return error_page(lang, OauthError::FailedToListDatabases);
        }
    };

    // (display_name, caldav_username, plaintext password) for calendars
    // actually created just now — shown once on the dashboard (reveal-able
    // again later via caldav_password_encrypted, see encrypt_password).
    let mut new_credentials: Vec<(String, String, String)> = Vec::new();
    // Titles of databases already connected under *this* user's own account.
    // database_id is no longer globally unique (see migrations/0003 — the
    // same Notion database can now have many subscribers), so the only
    // remaining conflict is UNIQUE(user_id, database_id): re-selecting one
    // you already have. Surfaced on /me instead of failing silently.
    let mut already_connected: Vec<String> = Vec::new();

    for db_id in &form.db_ids {
        let Some(candidate) = candidates
            .iter()
            .find(|c| &c.database_id == db_id && c.date_property.is_some())
        else {
            continue;
        };
        let date_property = candidate.date_property.clone().expect("checked above");

        let public_id = uuid::Uuid::new_v4().to_string();
        let caldav_username = format!("cal_{}", generate_token(12));
        let caldav_password = generate_token(24);
        let password_hash = match hash_password(&caldav_password) {
            Ok(h) => h,
            Err(e) => {
                error!("failed to hash caldav password: {}", e);
                continue;
            }
        };
        let password_encrypted = state
            .password_enc_key
            .as_ref()
            .and_then(|key| encrypt_password(key, &caldav_password))
            .unwrap_or_default();

        let result = sqlx::query(
            "INSERT INTO calendars (user_id, notion_connection_id, database_id, public_id, data_source_id, date_property, display_name, caldav_username, caldav_password_hash, caldav_password_encrypted)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, database_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(form.connection_id)
        .bind(&candidate.database_id)
        .bind(&public_id)
        .bind(&candidate.data_source_id)
        .bind(&date_property)
        .bind(&candidate.title)
        .bind(&caldav_username)
        .bind(&password_hash)
        .bind(&password_encrypted)
        .execute(&state.db)
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => {
                new_credentials.push((candidate.title.clone(), caldav_username, caldav_password))
            }
            Ok(_) => already_connected.push(candidate.title.clone()),
            Err(e) => error!("failed to insert calendar {}: {}", candidate.database_id, e),
        }
    }

    if !new_credentials.is_empty() {
        if let Err(e) = session
            .insert("new_calendar_credentials", &new_credentials)
            .await
        {
            error!("failed to stash new calendar credentials in session: {}", e);
        }
        state.refresh_all().await;
    }

    if !already_connected.is_empty() {
        if let Err(e) = session
            .insert("calendar_connect_errors", &already_connected)
            .await
        {
            error!("failed to stash calendar connect errors in session: {}", e);
        }
    }

    Redirect::to("/me").into_response()
}

/// Ownership-checked lookup shared by the delete/reveal/regenerate actions
/// below — same shape as webview.rs's `require_owned_calendar`, kept as its
/// own small copy here since oauth.rs doesn't depend on webview.rs.
async fn owned_calendar_or_error(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
    public_id: &str,
    lang: crate::i18n::Lang,
) -> Result<crate::caldav::CalendarRow, axum::response::Response> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return Err(error_page(lang, OauthError::Generic)),
    };
    match state.calendar_by_public_id(public_id).await {
        Some(cal) if cal.user_id == user_id => Ok(cal),
        _ => Err(error_page(lang, OauthError::CalendarNotFound)),
    }
}

/// Removes a calendar subscription (the SaaS's own row only — the
/// underlying Notion database/pages are untouched). Evicts the shared cache
/// entry too, but only once no other subscriber still references the same
/// database_id.
pub async fn delete_calendar(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    if let Err(e) = sqlx::query("DELETE FROM calendars WHERE id = $1")
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to delete calendar {}: {}", cal.id, e);
        return error_page(lang, OauthError::FailedToDeleteCalendar);
    }

    let still_referenced: i64 =
        sqlx::query_scalar("SELECT count(*) FROM calendars WHERE database_id = $1")
            .bind(&cal.database_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(1);
    if still_referenced == 0 {
        state.cache.write().await.remove(&cal.database_id);
    }

    Redirect::to("/me").into_response()
}

/// Decrypts and re-surfaces a calendar's current CalDAV password via the
/// same one-time session stash `create_calendars` uses — reuses `me()`'s
/// existing "just created" display path rather than a separate UI state.
/// Rows created before `caldav_password_encrypted` existed (or before
/// `CALDAV_PASSWORD_ENC_KEY` was configured) have nothing recoverable here;
/// "Tạo lại mật khẩu" is the only way to get a working password shown again.
pub async fn reveal_password(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let Some(key) = state.password_enc_key.as_ref() else {
        return error_page(lang, OauthError::RevealPasswordNotConfigured);
    };

    let encrypted: String =
        sqlx::query_scalar("SELECT caldav_password_encrypted FROM calendars WHERE id = $1")
            .bind(cal.id)
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();

    let Some(password) = (!encrypted.is_empty())
        .then(|| decrypt_password(key, &encrypted))
        .flatten()
    else {
        return error_page(lang, OauthError::PasswordPredatesReveal);
    };

    let stash = vec![(
        cal.display_name.clone(),
        cal.caldav_username.clone(),
        password,
    )];
    if let Err(e) = session.insert("new_calendar_credentials", &stash).await {
        error!("failed to stash revealed password in session: {}", e);
    }

    Redirect::to("/me").into_response()
}

/// Issues a brand new CalDAV password for a calendar, invalidating the old
/// one — for rows whose original password isn't recoverable (see
/// `reveal_password`), or just as a routine credential rotation.
pub async fn regenerate_password(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let new_password = generate_token(24);
    let Ok(password_hash) = hash_password(&new_password) else {
        return error_page(lang, OauthError::Generic);
    };
    let password_encrypted = state
        .password_enc_key
        .as_ref()
        .and_then(|key| encrypt_password(key, &new_password))
        .unwrap_or_default();

    if let Err(e) = sqlx::query("UPDATE calendars SET caldav_password_hash = $1, caldav_password_encrypted = $2 WHERE id = $3")
        .bind(&password_hash)
        .bind(&password_encrypted)
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to regenerate caldav password for calendar {}: {}", cal.id, e);
        return error_page(lang, OauthError::FailedToRegeneratePassword);
    }

    let stash = vec![(
        cal.display_name.clone(),
        cal.caldav_username.clone(),
        new_password,
    )];
    if let Err(e) = session.insert("new_calendar_credentials", &stash).await {
        error!("failed to stash regenerated password in session: {}", e);
    }

    Redirect::to("/me").into_response()
}

/// Reads and clears the one-time post-onboarding credential stash written by
/// `create_calendars` (and by `reveal_password`/`regenerate_password`),
/// keyed by caldav_username for `me()` to display.
pub async fn take_new_calendar_credentials(
    session: &tower_sessions::Session,
) -> HashMap<String, String> {
    let stashed: Vec<(String, String, String)> = session
        .get("new_calendar_credentials")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if !stashed.is_empty() {
        let _ = session
            .remove::<Vec<(String, String, String)>>("new_calendar_credentials")
            .await;
    }
    stashed
        .into_iter()
        .map(|(_, username, password)| (username, password))
        .collect()
}

/// Reads and clears the one-time stash of database titles that were already
/// connected under this same account (see `create_calendars`) — `me()` shows
/// these as a notice instead of the previous silent no-op.
pub async fn take_calendar_connect_errors(session: &tower_sessions::Session) -> Vec<String> {
    let stashed: Vec<String> = session
        .get("calendar_connect_errors")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if !stashed.is_empty() {
        let _ = session
            .remove::<Vec<String>>("calendar_connect_errors")
            .await;
    }
    stashed
}

#[derive(sqlx::FromRow)]
struct SyncLogRow {
    occurred_at: String,
    source: String,
    action: String,
    event_uid: String,
    notion_page_id: String,
    status: String,
    detail: String,
}

const SYNC_LOG_STYLE: &str = r#"
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1.25rem; line-height: 1.5; color: #1a1a1a; }
  .top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.25rem; }
  .top-nav a.back { font-size: 0.85rem; color: #666; text-decoration: none; }
  h1 { margin: 0.25rem 0 1.25rem; font-size: 1.3rem; }
  table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }
  th, td { text-align: left; padding: 0.5rem 0.6rem; border-bottom: 1px solid #eee; vertical-align: top; }
  th { color: #888; font-weight: 500; font-size: 0.78rem; text-transform: uppercase; letter-spacing: 0.02em; }
  .status-ok { color: #166534; font-weight: 600; }
  .status-error { color: #991b1b; font-weight: 600; }
  .source-badge { display: inline-block; padding: 0.1rem 0.5rem; border-radius: 6px; font-size: 0.75rem; background: #f1f1f1; }
  code { font-family: ui-monospace, monospace; background: #f1f1f1; padding: 0.1rem 0.35rem; border-radius: 4px; font-size: 0.78rem; word-break: break-all; }
  .detail-cell { max-width: 320px; white-space: pre-wrap; word-break: break-word; color: #991b1b; }
  .empty { color: #888; padding: 2rem 0; text-align: center; }
</style>
"#;

/// Shows the last 200 create/update/delete attempts for one calendar
/// (whether via CalDAV client or the webview), newest first — so a user can
/// self-diagnose sync issues (did the write reach the server? did it reach
/// Notion? what error?) from a browser instead of asking someone to read
/// the application's raw logs. See AppState::log_sync for what gets
/// recorded and from where.
pub async fn sync_log_page(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let rows: Vec<SyncLogRow> = sqlx::query_as(
        "SELECT occurred_at::text AS occurred_at, source, action, event_uid, notion_page_id, status, detail
         FROM sync_log WHERE calendar_id = $1 ORDER BY occurred_at DESC LIMIT 200",
    )
    .bind(cal.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| {
        error!("failed to load sync_log for calendar {}: {}", cal.id, e);
        Vec::new()
    });

    let name = html_escape(&cal.display_name);
    let rows_html = if rows.is_empty() {
        r#"<tr><td colspan="6" class="empty">Chưa có hoạt động đồng bộ nào được ghi lại.</td></tr>"#
            .to_string()
    } else {
        rows.iter()
            .map(|r| {
                let status_class = if r.status == "ok" {
                    "status-ok"
                } else {
                    "status-error"
                };
                let status_label = if r.status == "ok" { "OK" } else { "Lỗi" };
                let page_link = if r.notion_page_id.is_empty() {
                    "—".to_string()
                } else {
                    format!(
                        r#"<a href="https://notion.so/{id}" target="_blank">{short}</a>"#,
                        id = html_escape(&r.notion_page_id.replace('-', "")),
                        short = html_escape(&r.notion_page_id.chars().take(8).collect::<String>())
                    )
                };
                format!(
                    r#"<tr>
  <td>{time}</td>
  <td><span class="source-badge">{source}</span></td>
  <td>{action}</td>
  <td><code>{uid}</code></td>
  <td>{page_link}</td>
  <td><span class="{status_class}">{status_label}</span>{detail}</td>
</tr>"#,
                    time = html_escape(&r.occurred_at),
                    source = html_escape(&r.source),
                    action = html_escape(&r.action),
                    uid = if r.event_uid.is_empty() {
                        "—".to_string()
                    } else {
                        html_escape(&r.event_uid)
                    },
                    page_link = page_link,
                    status_class = status_class,
                    status_label = status_label,
                    detail = if r.detail.is_empty() {
                        String::new()
                    } else {
                        format!(
                            r#"<div class="detail-cell">{}</div>"#,
                            html_escape(&r.detail)
                        )
                    },
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Log đồng bộ — {name}</title>{SYNC_LOG_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="back" href="/me">← Tất cả lịch</a></div>
<h1>Log đồng bộ — {name}</h1>
<table>
  <thead>
    <tr><th>Thời gian</th><th>Nguồn</th><th>Hành động</th><th>UID sự kiện</th><th>Notion page</th><th>Kết quả</th></tr>
  </thead>
  <tbody>
{rows_html}
  </tbody>
</table>
</body></html>"#
    ))
    .into_response()
}
