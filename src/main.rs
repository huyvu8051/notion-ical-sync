use std::{
    env,
    time::Duration,
};
use tower_sessions::cookie::time::Duration as CookieDuration;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use notion_ical_sync::{auth, oauth, AppState, CaldavAllowWrites, create_app};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let webhook_secret = env::var("NOTION_WEBHOOK_SECRET").ok();
    if webhook_secret.is_none() {
        tracing::warn!(
            "NOTION_WEBHOOK_SECRET not set; webhook events will be logged but ignored \
             (signature can't be verified) until it's configured"
        );
    }

    // Notion credentials/database selection are no longer flat env vars —
    // each tenant's own token + calendar list lives in Postgres (see
    // migrations/0001_init.sql: users -> notion_connections -> calendars).
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://biolink:biolink@localhost:5433/notion_saas".to_string());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to postgres");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    let app_base_url = env::var("APP_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));
    let notion_oauth = oauth::NotionOAuthConfig::from_env(&app_base_url);
    if notion_oauth.is_none() {
        tracing::warn!(
            "NOTION_OAUTH_CLIENT_ID/NOTION_OAUTH_CLIENT_SECRET not set; the \"Connect Notion\" \
             onboarding flow will show a not-configured page until they're set"
        );
    }

    // Lets the dashboard's "Hiện mật khẩu" action decrypt a calendar's
    // stored CalDAV password again later (see oauth.rs::encrypt_password).
    // Must decode to exactly 32 bytes (AES-256-GCM) — generate with e.g.
    // `openssl rand -base64 32`. None just disables reveal (rows show "Tạo
    // lại mật khẩu" instead), same posture as the other optional configs.
    let password_enc_key = env::var("CALDAV_PASSWORD_ENC_KEY")
        .ok()
        .and_then(|b64| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(b64).ok()
        })
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    if password_enc_key.is_none() {
        tracing::warn!(
            "CALDAV_PASSWORD_ENC_KEY not set (or not valid base64 for 32 bytes); \"Hiện mật khẩu\" \
             will be unavailable until it's configured — \"Tạo lại mật khẩu\" still works"
        );
    }

    // $1/year Stripe subscription checkout. None disables /billing/checkout
    // (shows a not-configured page), same posture as the other optional
    // integrations above.
    let stripe = notion_ical_sync::billing::StripeConfig::from_env();
    if stripe.is_none() {
        tracing::warn!(
            "STRIPE_SECRET_KEY/STRIPE_WEBHOOK_SECRET/STRIPE_PRICE_ID not set; \
             /billing/checkout will show a not-configured page until they're set"
        );
    }

    let caldav_allow_writes = CaldavAllowWrites::from_env();
    let state = AppState::new(pool.clone(), caldav_allow_writes, webhook_secret, notion_oauth, password_enc_key, stripe);

    // Initial refresh
    state.refresh_all().await;

    // Periodic refresh: every 10 minutes. Webhooks (see webhook.rs) cover the
    // realtime case by refreshing the affected database immediately, so this
    // poll is now just the fallback/catch-up path for anything a webhook missed.
    let state2 = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(600));
        loop {
            interval.tick().await;
            state2.refresh_all().await;
        }
    });

    // SaaS login (Keycloak) — separate from the per-tenant Notion OAuth
    // tokens already sitting in Postgres. Sessions are Postgres-backed (not
    // MemoryStore) since this app is multi-tenant from day one.
    let keycloak_issuer_url = env::var("KEYCLOAK_ISSUER_URL")
        .unwrap_or_else(|_| "http://localhost:8081/realms/notion-caldav-saas".to_string());
    let keycloak_client_id =
        env::var("KEYCLOAK_CLIENT_ID").unwrap_or_else(|_| "notion-caldav-saas-app".to_string());
    let keycloak_client_secret = env::var("KEYCLOAK_CLIENT_SECRET").ok();

    let oidc_client = auth::build_oidc_client(
        keycloak_issuer_url,
        keycloak_client_id,
        keycloak_client_secret,
        format!("{app_base_url}/oidc"),
    )
    .await;

    let session_store = PostgresStore::new(pool);
    session_store.migrate().await.expect("failed to run session store migrations");
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(app_base_url.starts_with("https://"))
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(CookieDuration::days(7)));

    let app_config = auth::AppConfig { base_url: app_base_url };

    let app = create_app(state, oidc_client, app_config).layer(session_layer);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("notion-ical-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
