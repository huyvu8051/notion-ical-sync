use crate::AppState;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum_oidc::openidconnect::core::CoreGenderClaim;
use axum_oidc::openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims, OidcClient, OidcSession};

#[derive(Clone)]
pub struct AppConfig {
    pub base_url: String,
}

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

    async fn get(
        &self,
    ) -> Result<OidcSession<EmptyAdditionalClaims, CoreGenderClaim>, Self::Error> {
        Ok(self.0.get("axum-oidc").await?.unwrap_or_default())
    }

    async fn set(
        &mut self,
        value: OidcSession<EmptyAdditionalClaims, CoreGenderClaim>,
    ) -> Result<(), Self::Error> {
        self.0.insert("axum-oidc", value).await?;
        Ok(())
    }
}

const OIDC_DISCOVERY_MAX_ATTEMPTS: u32 = 8;
const OIDC_DISCOVERY_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);
const OIDC_DISCOVERY_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn build_oidc_client_with_startup_retry(
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_url: String,
) -> OidcClient<EmptyAdditionalClaims> {
    let mut backoff = OIDC_DISCOVERY_INITIAL_BACKOFF;
    for attempt in 1..=OIDC_DISCOVERY_MAX_ATTEMPTS {
        let mut builder = OidcClient::<EmptyAdditionalClaims>::builder()
            .with_default_http_client()
            .with_redirect_url(
                redirect_url
                    .parse()
                    .unwrap_or_else(|_| panic!("invalid redirect url: {redirect_url}")),
            )
            .with_client_id(ClientId::new(client_id.clone()))
            .add_scope(Scope::new("profile".to_string()))
            .add_scope(Scope::new("email".to_string()));

        if let Some(secret) = client_secret.clone() {
            builder = builder.with_client_secret(ClientSecret::new(secret));
        }

        let issuer_url = IssuerUrl::new(issuer.clone()).expect("invalid KEYCLOAK_ISSUER_URL");
        match builder.discover(issuer_url).await {
            Ok(builder) => return builder.build(),
            Err(e) if attempt < OIDC_DISCOVERY_MAX_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    error = %e,
                    ?backoff,
                    "Keycloak OIDC discovery failed, retrying"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(OIDC_DISCOVERY_MAX_BACKOFF);
            }
            Err(e) => panic!(
                "failed to discover Keycloak OIDC issuer after {OIDC_DISCOVERY_MAX_ATTEMPTS} attempts — is it running? {e}"
            ),
        }
    }
    unreachable!("loop always returns or panics on the last attempt")
}

pub async fn find_or_create_user(
    state: &AppState,
    keycloak_sub: &str,
    email: &str,
) -> Result<i64, sqlx::Error> {
    let (id, inserted, has_account_credential): (i64, bool, bool) = sqlx::query_as(
        "INSERT INTO users (keycloak_sub, email) VALUES ($1, $2)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id, (xmax = 0) AS inserted, account_caldav_username IS NOT NULL AS has_account_credential",
    )
    .bind(keycloak_sub)
    .bind(email)
    .fetch_one(&state.db)
    .await?;

    if inserted {
        if let Some(cfg) = state.email.clone() {
            let (subject, html) = crate::email::welcome_email();
            crate::email::spawn_send(cfg, email.to_string(), subject.to_string(), html);
        }
    }

    if !has_account_credential {
        ensure_account_caldav_credential(&state.db, id).await;
    }

    Ok(id)
}

async fn ensure_account_caldav_credential(pool: &sqlx::PgPool, user_id: i64) {
    let username = format!("acct_{}", crate::crypto::generate_token(12));
    let password = crate::crypto::generate_token(24);
    let token = crate::crypto::generate_token(32);
    let Ok(hash) = crate::crypto::hash_password(&password) else {
        return;
    };
    let _ = sqlx::query(
        "UPDATE users SET account_caldav_username = $1, account_caldav_password_hash = $2, account_caldav_password = $3, mobileconfig_token = $4
         WHERE id = $5 AND account_caldav_username IS NULL",
    )
    .bind(&username)
    .bind(&hash)
    .bind(&password)
    .bind(&token)
    .bind(user_id)
    .execute(pool)
    .await;
}

pub async fn current_user_id(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
) -> Result<i64, axum::http::StatusCode> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    find_or_create_user(state, sub, email)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn require_owned_calendar(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
    public_id: &str,
) -> Result<crate::caldav::CalendarRow, axum::http::StatusCode> {
    let user_id = current_user_id(state, claims).await?;
    match state.calendar_by_public_id(public_id).await {
        Some(cal) if cal.user_id == user_id => Ok(cal),
        Some(_) => Err(axum::http::StatusCode::FORBIDDEN),
        None => Err(axum::http::StatusCode::NOT_FOUND),
    }
}

pub async fn owned_calendar_or_error(
    state: &AppState,
    claims: &OidcClaims<EmptyAdditionalClaims>,
    public_id: &str,
) -> Result<crate::caldav::CalendarRow, axum::response::Response> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(state, sub, &email).await {
        Ok(id) => id,
        Err(_) => {
            return Err(crate::error_page::error_page(
                crate::error_page::OauthError::Generic,
            ))
        }
    };
    match state.calendar_by_public_id(public_id).await {
        Some(cal) if cal.user_id == user_id => Ok(cal),
        _ => Err(crate::error_page::error_page(
            crate::error_page::OauthError::CalendarNotFound,
        )),
    }
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub async fn logout(
    logout: axum_oidc::OidcRpInitiatedLogout,
    State(state): State<AppState>,
    cfg: axum::Extension<AppConfig>,
    session: tower_sessions::Session,
) -> impl axum::response::IntoResponse {
    let _ = &state;
    if let Err(e) = session.flush().await {
        tracing::warn!("logout: failed to flush session: {}", e);
    }
    let redirect_uri = cfg
        .base_url
        .parse()
        .unwrap_or_else(|_| panic!("invalid APP_BASE_URL: {}", cfg.base_url));
    logout.with_post_logout_redirect(redirect_uri)
}
