use notion_ical_sync::{auth, billing, create_app, email, oauth, AppState, CaldavAllowWrites};
use std::{env, time::Duration};
use tower_sessions::cookie::time::Duration as CookieDuration;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const JOB_LOCK_REFRESH_ALL: i64 = 90001;
const JOB_LOCK_TRIAL_REMINDERS: i64 = 90002;

const PERIODIC_REFRESH_INTERVAL: Duration = Duration::from_secs(600);
const DAILY_TRIAL_REMINDER_INTERVAL: Duration = Duration::from_secs(86400);

async fn advisory_lock(
    pool: &sqlx::PgPool,
    key: i64,
) -> Option<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await.ok()?;
    let (locked,): (bool,) = sqlx::query_as("SELECT pg_try_advisory_xact_lock($1)")
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .ok()?;
    locked.then_some(tx)
}

fn warn_if_unconfigured<T>(value: &Option<T>, message: &str) {
    if value.is_none() {
        tracing::warn!("{}", message);
    }
}

async fn run_periodic_refresh_job(state: AppState) {
    let mut ticker = tokio::time::interval(PERIODIC_REFRESH_INTERVAL);
    loop {
        ticker.tick().await;
        if let Some(lock) = advisory_lock(&state.db, JOB_LOCK_REFRESH_ALL).await {
            state.refresh_all().await;
            let _ = lock.commit().await;
        }
    }
}

async fn run_daily_trial_reminder_job(state: AppState) {
    let mut ticker = tokio::time::interval(DAILY_TRIAL_REMINDER_INTERVAL);
    loop {
        ticker.tick().await;
        if let Some(lock) = advisory_lock(&state.db, JOB_LOCK_TRIAL_REMINDERS).await {
            billing::send_trial_reminders(&state).await;
            let _ = lock.commit().await;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    app::init_executor();

    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let webhook_secret = env::var("NOTION_WEBHOOK_SECRET").ok();
    warn_if_unconfigured(
        &webhook_secret,
        "NOTION_WEBHOOK_SECRET not set; webhook events will be logged but ignored \
         (signature can't be verified) until it's configured",
    );

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

    let app_base_url =
        env::var("APP_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));
    let notion_oauth = oauth::NotionOAuthConfig::from_env(&app_base_url);
    warn_if_unconfigured(
        &notion_oauth,
        "NOTION_OAUTH_CLIENT_ID/NOTION_OAUTH_CLIENT_SECRET not set; the \"Connect Notion\" \
         onboarding flow will show a not-configured page until they're set",
    );

    let password_enc_key = env::var("CALDAV_PASSWORD_ENC_KEY")
        .ok()
        .and_then(|b64| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(b64).ok()
        })
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    warn_if_unconfigured(
        &password_enc_key,
        "CALDAV_PASSWORD_ENC_KEY not set (or not valid base64 for 32 bytes); \"Hiện mật khẩu\" \
         will be unavailable until it's configured — \"Tạo lại mật khẩu\" still works",
    );

    let stripe = billing::StripeConfig::from_env();
    warn_if_unconfigured(
        &stripe,
        "STRIPE_SECRET_KEY/STRIPE_WEBHOOK_SECRET/STRIPE_PRICE_ID not set; \
         /billing/checkout will show a not-configured page until they're set",
    );

    let email = email::EmailConfig::from_env();
    warn_if_unconfigured(
        &email,
        "SMTP_HOST/SMTP_PORT/SMTP_USERNAME/SMTP_PASSWORD/EMAIL_FROM not set; \
         transactional emails will not be sent until they're configured",
    );

    let admin_secret = env::var("ADMIN_SECRET").ok();
    warn_if_unconfigured(
        &admin_secret,
        "ADMIN_SECRET not set; /admin/reset-billing is disabled",
    );

    let caldav_allow_writes = CaldavAllowWrites::from_env();
    let state = AppState::new(
        pool.clone(),
        caldav_allow_writes,
        webhook_secret,
        notion_oauth,
        password_enc_key,
        stripe,
        email,
        admin_secret,
    );

    state.refresh_all().await;
    tokio::spawn(run_periodic_refresh_job(state.clone()));
    tokio::spawn(run_daily_trial_reminder_job(state.clone()));

    let keycloak_issuer_url = env::var("KEYCLOAK_ISSUER_URL")
        .unwrap_or_else(|_| "http://localhost:8081/realms/notion-caldav-saas".to_string());
    let keycloak_client_id =
        env::var("KEYCLOAK_CLIENT_ID").unwrap_or_else(|_| "notion-caldav-saas-app".to_string());
    let keycloak_client_secret = env::var("KEYCLOAK_CLIENT_SECRET").ok();

    let oidc_client = auth::build_oidc_client_with_startup_retry(
        keycloak_issuer_url,
        keycloak_client_id,
        keycloak_client_secret,
        format!("{app_base_url}/oidc"),
    )
    .await;

    let session_store = PostgresStore::new(pool);
    session_store
        .migrate()
        .await
        .expect("failed to run session store migrations");
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(app_base_url.starts_with("https://"))
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(CookieDuration::days(7)));

    let app_config = auth::AppConfig {
        base_url: app_base_url,
    };

    let app = create_app(state, oidc_client, app_config).layer(session_layer);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("notion-ical-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
