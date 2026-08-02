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
        r#"<div class="banner-success">Đã kết nối thành công! Lịch của bạn đã sẵn sàng. Lưu lại mật khẩu CalDAV bên dưới — chúng tôi sẽ không hiển thị lại.</div>"#
    } else {
        ""
    };

    let items: String = calendars
        .iter()
        .map(|(db_id, name, caldav_username)| {
            let label = if name.is_empty() { db_id.as_str() } else { name.as_str() };
            let caldav_url = format!("{}/cal/{}", cfg.base_url, db_id);
            let password_row = match new_passwords.get(caldav_username) {
                Some(pw) => format!(
                    r#"<div class="cred-row"><span class="cred-label">Mật khẩu (chỉ hiện 1 lần):</span> <code>{}</code></div>"#,
                    html_escape(pw)
                ),
                None => String::new(),
            };
            format!(
                r#"<li class="cal-card">
                    <div class="cal-card-title">{label}</div>
                    <div class="cred-row"><span class="cred-label">URL:</span> <code>{url}</code></div>
                    <div class="cred-row"><span class="cred-label">Username:</span> <code>{username}</code></div>
                    {password_row}
                    <a href="/app/{db_id}">Mở lịch</a>
                </li>"#,
                label = html_escape(label),
                url = html_escape(&caldav_url),
                username = html_escape(caldav_username),
                db_id = html_escape(db_id),
            )
        })
        .collect();

    let content = if calendars.is_empty() {
        r#"<p class="hint">Chưa có calendar nào — kết nối Notion để bắt đầu.</p>"#.to_string()
    } else {
        format!(r#"<ul class="cal-list">{items}</ul>"#)
    };

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Trang của bạn — Notion CalDAV SaaS</title>{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/logout">Đăng xuất</a></div>
{banner}
<div class="header-row"><h1>Calendar của bạn</h1><a class="connect-btn-secondary" href="/connect/notion">+ Kết nối thêm cơ sở dữ liệu</a></div>
{content}
</body></html>"#
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
