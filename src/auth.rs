//! Keycloak/OIDC wiring for the SaaS's own login (separate from the Notion
//! OAuth in oauth.rs, which is *this app's* permission to read/write a
//! user's Notion workspace — two different identities, don't conflate them).
//! Pattern copied from biolink-vn/src/auth.rs, which has the same axum-oidc +
//! tower-sessions shape.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::openidconnect::core::CoreGenderClaim;
use axum_oidc::openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims, OidcClient, OidcRpInitiatedLogout, OidcSession};

use crate::AppState;

#[derive(Clone)]
pub struct AppConfig {
    pub base_url: String,
}

/// Bridges axum-oidc's session trait to the tower-sessions cookie session —
/// boilerplate required by the crate, not app-specific logic.
pub struct SessionWrapper(pub tower_sessions::Session);

impl<S: Send + Sync> FromRequestParts<S> for SessionWrapper {
    type Rejection = <tower_sessions::Session as FromRequestParts<S>>::Rejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = tower_sessions::Session::from_request_parts(parts, state).await?;
        Ok(Self(session))
    }
}

impl axum_oidc::Session<EmptyAdditionalClaims> for SessionWrapper {
    type Error = tower_sessions::session::Error;

    async fn get(&self) -> Result<OidcSession<EmptyAdditionalClaims, CoreGenderClaim>, Self::Error> {
        Ok(self.0.get("axum-oidc").await?.unwrap_or_default())
    }

    async fn set(&mut self, value: OidcSession<EmptyAdditionalClaims, CoreGenderClaim>) -> Result<(), Self::Error> {
        self.0.insert("axum-oidc", value).await?;
        Ok(())
    }
}

pub async fn build_oidc_client(
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_url: String,
) -> OidcClient<EmptyAdditionalClaims> {
    let mut builder = OidcClient::<EmptyAdditionalClaims>::builder()
        .with_default_http_client()
        .with_redirect_url(
            redirect_url
                .parse()
                .unwrap_or_else(|_| panic!("invalid redirect url: {redirect_url}")),
        )
        .with_client_id(ClientId::new(client_id))
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()));

    if let Some(secret) = client_secret {
        builder = builder.with_client_secret(ClientSecret::new(secret));
    }

    builder
        .discover(IssuerUrl::new(issuer).expect("invalid KEYCLOAK_ISSUER_URL"))
        .await
        .expect("failed to discover Keycloak OIDC issuer — is it running?")
        .build()
}

/// Find-or-create the `users` row for this Keycloak login, returning its id.
pub async fn find_or_create_user(pool: &sqlx::PgPool, keycloak_sub: &str, email: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (keycloak_sub, email) VALUES ($1, $2)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id",
    )
    .bind(keycloak_sub)
    .bind(email)
    .fetch_one(pool)
    .await
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) const AUTH_STYLE: &str = r#"
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, sans-serif; max-width: 480px; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.5; }
  .top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 2rem; }
  .top-nav a.logout { font-size: 0.85rem; color: #666; text-decoration: none; }
  .cal-list { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 0.75rem; }
  .cal-card { display: block; padding: 0.9rem 1rem; background: #f6f6f6; border-radius: 12px; }
  .cal-card-title { font-weight: 600; margin-bottom: 0.4rem; }
  .cal-card a { color: #2563eb; text-decoration: none; }
  .hint { color: #666; font-size: 0.9rem; }
  .header-row { display: flex; justify-content: space-between; align-items: center; gap: 1rem; margin-bottom: 1.5rem; }
  .header-row h1 { margin: 0; }
  .connect-btn, .connect-btn-secondary { display: inline-block; padding: 0.55rem 1.1rem; border-radius: 8px; text-decoration: none; font-size: 0.9rem; cursor: pointer; border: none; font-family: inherit; }
  .connect-btn { background: #171717; color: #fff; margin-top: 1rem; }
  .connect-btn-secondary { border: 1px solid #ddd; color: #171717; background: #fff; white-space: nowrap; }
  .cred-row { font-size: 0.85rem; color: #444; margin: 0.15rem 0; }
  .cred-label { color: #888; margin-right: 0.35rem; }
  .banner-success { background: #dcfce7; color: #166534; padding: 0.75rem 1rem; border-radius: 8px; margin-bottom: 1.25rem; font-size: 0.9rem; }
  .banner-error { background: #fee2e2; color: #991b1b; padding: 0.75rem 1rem; border-radius: 8px; margin-bottom: 1.25rem; font-size: 0.9rem; }
  code { font-family: ui-monospace, monospace; background: #eee; padding: 0.1rem 0.35rem; border-radius: 4px; font-size: 0.85rem; }
  .connect-card { margin-top: 2rem; }
  .reassure-list { list-style: none; padding: 0; margin-top: 1.5rem; font-size: 0.85rem; color: #555; }
  .reassure-list li { margin: 0.35rem 0; }
  .reassure-list li::before { content: "✓ "; color: #16a34a; }
  .db-list { display: flex; flex-direction: column; gap: 0.6rem; margin: 1.25rem 0; }
  .db-card { display: flex; align-items: center; gap: 0.6rem; padding: 0.75rem 1rem; border: 1px solid #e5e5e5; border-radius: 8px; cursor: pointer; font-size: 0.95rem; }
  .db-card-disabled { opacity: 0.5; cursor: not-allowed; }
  .db-name { font-weight: 500; }
  .db-meta { color: #888; font-size: 0.8rem; margin-left: auto; }
  .db-warning { color: #ba1a1a; font-size: 0.8rem; margin-left: auto; }
  .action-bar { display: flex; justify-content: space-between; margin-top: 1.5rem; }
</style>
"#;

/// `/me` (dashboard) is styled to match the Stitch "Calendar của bạn" mockup
/// (Stitch project 7966553897766226544, screen 14ecf17308d644d68daa28eb5f3c50a0)
/// pixel-for-pixel — Tailwind CDN + the exact design-token config from that
/// screen, rather than the plain hand-rolled CSS the rest of the app uses.
pub(crate) const DASHBOARD_HEAD: &str = r##"
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&display=swap" rel="stylesheet">
<link href="https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet">
<script id="tailwind-config">
tailwind.config = {
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "error": "#ba1a1a", "surface-container-low": "#f5f3f3", "surface-container-high": "#e9e8e7",
        "surface-container-highest": "#e3e2e2", "on-surface-variant": "#444748"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  }
}
</script>
<style>
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.success-banner-gradient { background: linear-gradient(90deg, rgba(220, 252, 231, 0.5) 0%, rgba(220, 252, 231, 0.2) 100%); }
.error-banner-gradient { background: linear-gradient(90deg, rgba(254, 226, 226, 0.5) 0%, rgba(254, 226, 226, 0.2) 100%); }
</style>
"##;

/// Post-login landing: lists the user's own calendars, with a CTA to connect
/// more Notion databases (see oauth.rs). Doubles as the "onboarding
/// complete" screen right after `create_calendars` redirects here.
pub async fn me(
    claims: OidcClaims<EmptyAdditionalClaims>,
    State(state): State<AppState>,
    session: tower_sessions::Session,
    cfg: axum::Extension<AppConfig>,
) -> impl IntoResponse {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();

    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(_) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Có lỗi xảy ra.").into_response(),
    };

    let calendars: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT database_id, display_name, caldav_username FROM calendars WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Plaintext CalDAV passwords only ever exist for one request — we store
    // just a hash — so a calendar just created by create_calendars stashes
    // its password here in the session for this one render.
    let new_passwords = crate::oauth::take_new_calendar_credentials(&session).await;
    let banner = if !new_passwords.is_empty() {
        r#"<div class="flex items-center gap-sm p-md success-banner-gradient border border-[#DCFCE7] rounded-lg" id="success-banner">
<div class="flex items-center justify-center w-6 h-6 bg-[#DCFCE7] text-[#166534] rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">check_circle</span>
</div>
<p class="text-[#166534] font-medium text-body-md">Đã kết nối thành công! Lịch của bạn đã sẵn sàng. Lưu lại mật khẩu CalDAV bên dưới — chúng tôi sẽ không hiển thị lại.</p>
</div>"#.to_string()
    } else {
        String::new()
    };

    // A database_id can only ever belong to one calendar system-wide (see
    // migrations/0001_init.sql) — surface a clear reason here instead of the
    // previous silent no-op when someone tries to reconnect one that's
    // already claimed by a different account.
    let connect_errors = crate::oauth::take_calendar_connect_errors(&session).await;
    let error_banner = if connect_errors.is_empty() {
        String::new()
    } else {
        let names = connect_errors.iter().map(|n| html_escape(n)).collect::<Vec<_>>().join(", ");
        format!(
            r#"<div class="flex items-center gap-sm p-md error-banner-gradient border border-[#fecaca] rounded-lg">
<div class="flex items-center justify-center w-6 h-6 bg-[#fecaca] text-error rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">error</span>
</div>
<p class="text-error font-medium text-body-md">Không thể kết nối: <strong>{names}</strong> đã được kết nối bởi một tài khoản khác trên hệ thống này. Nếu đây là database của bạn, hãy đăng nhập bằng tài khoản đã kết nối trước đó, hoặc liên hệ hỗ trợ để chuyển quyền sở hữu.</p>
</div>"#
        )
    };

    fn copy_row(label: &str, value: &str) -> String {
        let escaped_value = html_escape(value);
        format!(
            r#"<div class="space-y-sm mt-sm">
<label class="font-label-md text-label-md text-on-surface-variant block uppercase tracking-wide">{label}</label>
<div class="flex gap-sm">
<input class="w-full h-10 px-md bg-surface-container-low border border-outline-variant font-code text-code focus:outline-none focus:ring-0 cursor-default" readonly type="text" value="{escaped_value}">
<button class="w-10 h-10 border border-outline-variant flex items-center justify-center hover:bg-surface-container-high transition-all active:bg-surface-container-highest shrink-0" onclick="copyToClipboard('{escaped_value}', this)">
<span class="material-symbols-outlined">content_copy</span>
</button>
</div>
</div>"#
        )
    }

    let items: String = calendars
        .iter()
        .map(|(db_id, name, caldav_username)| {
            let label = if name.is_empty() { db_id.as_str() } else { name.as_str() };
            let caldav_url = format!("{}/cal/{}", cfg.base_url, db_id);
            let password_row = match new_passwords.get(caldav_username) {
                Some(pw) => copy_row("Mật khẩu (chỉ hiện 1 lần)", pw),
                None => String::new(),
            };
            format!(
                r#"<div class="bg-surface border border-outline-variant rounded-lg p-lg hover:border-outline transition-colors duration-200">
<div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-md mb-lg">
<div class="flex items-center gap-sm">
<span class="text-h2">🗓️</span>
<h2 class="font-semibold text-h2">{label}</h2>
<span class="bg-[#DCFCE7] text-[#166534] px-xs py-[2px] rounded font-label-md text-[10px] uppercase tracking-wider">Đang hoạt động</span>
</div>
<a class="px-md h-8 border border-outline-variant hover:bg-surface-container-low font-label-md text-label-md transition-all flex items-center" href="/app/{db_id}">Mở lịch</a>
</div>
{url_row}
{username_row}
{password_row}
<p class="text-on-surface-variant text-[13px] mt-sm">Dán link này vào Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào</p>
</div>"#,
                label = html_escape(label),
                db_id = html_escape(db_id),
                url_row = copy_row("CalDAV URL", &caldav_url),
                username_row = copy_row("Username", caldav_username),
            )
        })
        .collect();

    let content = if calendars.is_empty() {
        r#"<div class="flex flex-col items-center justify-center py-xl text-center border border-dashed border-outline-variant rounded-lg">
<span class="material-symbols-outlined !text-[48px] text-outline mb-md">calendar_add_on</span>
<p class="text-body-lg text-on-surface-variant max-w-sm">Chưa có calendar nào — kết nối Notion để bắt đầu.</p>
</div>"#
            .to_string()
    } else {
        format!(r#"<div class="space-y-md">{items}</div>"#)
    };

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Trang của bạn — Notion CalDAV SaaS</title>{DASHBOARD_HEAD}</head>
<body class="bg-background text-on-surface font-body-md min-h-screen">
<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
<div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
<span class="text-h1 font-semibold tracking-tighter text-primary">Notion CalDAV SaaS</span>
<div class="flex items-center space-x-md">
<span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
<a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="Đăng xuất">
<span class="material-symbols-outlined">logout</span>
</a>
</div>
</div>
</header>
<main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
{banner}
{error_banner}
<div class="flex flex-col md:flex-row md:items-end justify-between gap-md border-b border-outline-variant pb-md">
<div>
<h1 class="text-h1 font-semibold">Calendar của bạn</h1>
<p class="text-on-surface-variant mt-1">Quản lý và đồng bộ hóa các cơ sở dữ liệu Notion với ứng dụng lịch yêu thích của bạn.</p>
</div>
<a class="bg-surface border border-outline-variant text-primary px-md h-10 font-label-md text-label-md flex items-center justify-center gap-sm hover:border-outline transition-all active:scale-95" href="/connect/notion">
<span class="material-symbols-outlined">add</span>
<span>Kết nối thêm cơ sở dữ liệu</span>
</a>
</div>
{content}
<p class="text-on-surface-variant text-[13px] pt-lg"><a class="underline hover:text-primary" href="/privacy">Privacy Policy</a> · <a class="underline hover:text-primary" href="/terms">Terms of Service</a></p>
</main>
<script>
function copyToClipboard(text, btn) {{
  navigator.clipboard.writeText(text).then(() => {{
    const icon = btn.querySelector('.material-symbols-outlined');
    const original = icon.innerText;
    icon.innerText = 'check';
    icon.classList.add('text-[#166534]');
    setTimeout(() => {{ icon.innerText = original; icon.classList.remove('text-[#166534]'); }}, 2000);
  }});
}}
</script>
</body></html>"#,
        email = html_escape(&email),
    ))
    .into_response()
}

pub async fn logout(logout: OidcRpInitiatedLogout, State(state): State<AppState>, cfg: axum::Extension<AppConfig>) -> impl IntoResponse {
    let _ = &state;
    let redirect_uri = cfg
        .base_url
        .parse()
        .unwrap_or_else(|_| panic!("invalid APP_BASE_URL: {}", cfg.base_url));
    logout.with_post_logout_redirect(redirect_uri)
}

/// Convenience for routes that just need "is anyone logged in" without
/// wanting the full claims — currently unused but kept small/available for
/// Phase 3's onboarding checks.
pub async fn redirect_root_to_me() -> Redirect {
    Redirect::to("/me")
}
