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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const AUTH_STYLE: &str = r#"
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, sans-serif; max-width: 480px; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.5; }
  .top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 2rem; }
  .top-nav a.logout { font-size: 0.85rem; color: #666; text-decoration: none; }
  .cal-list { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 0.75rem; }
  .cal-card { display: block; padding: 0.9rem 1rem; background: #f6f6f6; border-radius: 12px; text-decoration: none; color: inherit; }
  .hint { color: #666; font-size: 0.9rem; }
</style>
"#;

/// Post-login landing: lists the user's own calendars (empty for now until
/// Phase 3's Notion OAuth connect flow exists to actually create any).
pub async fn me(claims: OidcClaims<EmptyAdditionalClaims>, State(state): State<AppState>) -> impl IntoResponse {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();

    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(_) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Có lỗi xảy ra.").into_response(),
    };

    let calendars: Vec<(String, String)> = sqlx::query_as(
        "SELECT database_id, display_name FROM calendars WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: String = calendars
        .iter()
        .map(|(db_id, name)| {
            let label = if name.is_empty() { db_id.as_str() } else { name.as_str() };
            format!(
                r#"<li><a class="cal-card" href="/app/{}">{}</a></li>"#,
                html_escape(db_id),
                html_escape(label)
            )
        })
        .collect();

    let body = if calendars.is_empty() {
        r#"<p class="hint">Chưa có calendar nào — kết nối Notion để bắt đầu (sắp có).</p>"#.to_string()
    } else {
        format!(r#"<ul class="cal-list">{items}</ul>"#)
    };

    Html(format!(
        r#"<!doctype html>
<html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Trang của bạn — Notion CalDAV SaaS</title>{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>Notion CalDAV SaaS</strong><a class="logout" href="/logout">Đăng xuất</a></div>
<h1>Calendar của bạn</h1>
{body}
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
