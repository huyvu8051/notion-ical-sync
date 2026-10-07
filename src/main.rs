use notion_ical_sync::pages::connect_notion;
use notion_ical_sync::{billing, create_app, email, session, AppState, CaldavAllowWrites};
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
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

fn build_env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())
}

/// Holds the OTel providers alive for the process lifetime; their `Drop`
/// flushes any pending batch on the way down. Best-effort only — this
/// app doesn't hook a graceful-shutdown signal, so a SIGKILL skips this
/// and just loses the last few seconds of unflushed data, same tradeoff
/// already accepted for the DB pool.
struct OtelGuard {
    tracer_provider: opentelemetry_sdk::trace::SdkTracerProvider,
    logger_provider: opentelemetry_sdk::logs::SdkLoggerProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(e) = self.tracer_provider.shutdown() {
            eprintln!("failed to shut down OTel tracer provider: {e}");
        }
        if let Err(e) = self.logger_provider.shutdown() {
            eprintln!("failed to shut down OTel logger provider: {e}");
        }
    }
}

/// Wires up `tracing_subscriber` with stdout (as before) plus, when
/// `OTEL_EXPORTER_OTLP_ENDPOINT` is set, OTLP export of both spans (to
/// Tempo) and log events (to Loki) via the cluster's Grafana Alloy
/// collector. Unset (e.g. local dev) degrades to stdout-only, matching
/// every other optional integration in this file.
fn init_tracing() -> Option<OtelGuard> {
    let otel_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    let Some(_) = otel_endpoint else {
        tracing_subscriber::registry()
            .with(build_env_filter())
            .with(tracing_subscriber::fmt::layer())
            .init();
        tracing::warn!(
            "OTEL_EXPORTER_OTLP_ENDPOINT not set; traces/logs only go to stdout, not to Tempo/Loki"
        );
        return None;
    };

    let resource = opentelemetry_sdk::Resource::builder()
        .with_service_name("notion-caldav-saas")
        .build();

    // Must use the async-runtime-integrated processors (spawn their export
    // loop via `tokio::spawn` onto this already-running runtime) rather than
    // the default `.with_batch_exporter(...)` path, which spawns a plain
    // `std::thread` with no Tokio reactor — that thread panics ("there is no
    // reactor running") the moment the async reqwest client underneath the
    // OTLP exporter tries to do I/O. Learned this the hard way in production.
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP span exporter");
    let span_processor = opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(
        span_exporter,
        opentelemetry_sdk::runtime::Tokio,
    )
    .build();
    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(span_processor)
        .with_resource(resource.clone())
        .build();
    let tracer = tracer_provider.tracer("notion-caldav-saas");

    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP log exporter");
    let log_processor = opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor::builder(
        log_exporter,
        opentelemetry_sdk::runtime::Tokio,
    )
    .build();
    let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
        .with_log_processor(log_processor)
        .with_resource(resource)
        .build();
    let otel_log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    tracing_subscriber::registry()
        .with(build_env_filter())
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(otel_log_layer)
        .init();

    Some(OtelGuard {
        tracer_provider,
        logger_provider,
    })
}

async fn run_periodic_refresh_job(state: AppState) {
    let mut ticker = tokio::time::interval(PERIODIC_REFRESH_INTERVAL);
    loop {
        ticker.tick().await;
        info!("refresh_all: polling tick fired");
        match advisory_lock(&state.db, JOB_LOCK_REFRESH_ALL).await {
            Some(lock) => {
                state.refresh_all().await;
                let _ = lock.commit().await;
            }
            None => info!("refresh_all: tick skipped, another replica holds the lock"),
        }
    }
}

async fn run_daily_trial_reminder_job(state: AppState) {
    let mut ticker = tokio::time::interval(DAILY_TRIAL_REMINDER_INTERVAL);
    loop {
        ticker.tick().await;
        info!("trial_reminders: polling tick fired");
        match advisory_lock(&state.db, JOB_LOCK_TRIAL_REMINDERS).await {
            Some(lock) => {
                billing::send_trial_reminders(&state).await;
                let _ = lock.commit().await;
            }
            None => info!("trial_reminders: tick skipped, another replica holds the lock"),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let _otel_guard = init_tracing();

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
    let notion_oauth = connect_notion::NotionOAuthConfig::from_env(&app_base_url);
    warn_if_unconfigured(
        &notion_oauth,
        "NOTION_OAUTH_CLIENT_ID/NOTION_OAUTH_CLIENT_SECRET not set; the \"Connect Notion\" \
         onboarding flow will show a not-configured page until they're set",
    );

    let notion_api_base_url = env::var("NOTION_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.notion.com".to_string());
    if notion_api_base_url != "https://api.notion.com" {
        tracing::warn!(
            "NOTION_API_BASE_URL overridden to {} — pointing at a mock Notion, not the real API",
            notion_api_base_url
        );
    }

    let mapbox_token = env::var("MAPBOX_ACCESS_TOKEN").ok();
    warn_if_unconfigured(
        &mapbox_token,
        "MAPBOX_ACCESS_TOKEN not set; the event location field falls back to a plain text \
         input with no autocomplete",
    );

    let paddle = billing::PaddleConfig::from_env();
    warn_if_unconfigured(
        &paddle,
        "PADDLE_API_KEY/PADDLE_WEBHOOK_SECRET/PADDLE_PRICE_ID/PADDLE_CLIENT_TOKEN not set; \
         /billing/checkout will show a not-configured page until they're set",
    );

    let email = email::EmailConfig::from_env();
    warn_if_unconfigured(
        &email,
        "RESEND_API_KEY/EMAIL_FROM not set; \
         transactional emails will not be sent until they're configured",
    );

    let admin_secret = env::var("ADMIN_SECRET").ok();
    warn_if_unconfigured(
        &admin_secret,
        "ADMIN_SECRET not set; /admin/reset-billing is disabled",
    );

    for (key, default) in [
        ("LEPTOS_OUTPUT_NAME", "app"),
        ("LEPTOS_SITE_ROOT", "."),
        ("LEPTOS_SITE_PKG_DIR", "pkg"),
        ("LEPTOS_SITE_ADDR", &format!("127.0.0.1:{port}")),
        ("LEPTOS_RELOAD_PORT", "3002"),
    ] {
        if env::var(key).is_err() {
            env::set_var(key, default);
        }
    }
    let leptos_options = leptos::config::get_configuration(None)
        .expect("failed to load leptos config")
        .leptos_options;

    let caldav_allow_writes = CaldavAllowWrites::from_env();
    let state = AppState::new(
        pool.clone(),
        caldav_allow_writes,
        webhook_secret,
        notion_oauth,
        notion_api_base_url,
        mapbox_token,
        paddle,
        email,
        admin_secret,
        leptos_options,
    );

    // Replaces an old blocking `state.refresh_all().await` that used to sit
    // here, un-locked, run by every replica on every restart — with enough
    // tenant databases that routinely took 60+ seconds, well past the
    // liveness probe's threshold, so kubelet killed the pod before it ever
    // reached `axum::serve` below. That was the crash loop. This spawn is
    // non-blocking; `run_periodic_refresh_job`'s first tick fires right away
    // (confirmed: "refresh_all: pull cycle started" logged <1s after process
    // start), so this still covers the startup sync in the common case. The
    // advisory lock means only one replica's tick actually does the work
    // though — if that first attempt loses the lock (e.g. racing an outgoing
    // replica's own in-flight sweep during a rollout), it's silently skipped
    // and the next successful sync waits for the *next* scheduled tick
    // (~PERIODIC_REFRESH_INTERVAL later, ticks are evenly spaced from
    // startup, not re-tried immediately) — this is what happened on this
    // feature's first production deploy.
    tokio::spawn(run_periodic_refresh_job(state.clone()));
    tokio::spawn(run_daily_trial_reminder_job(state.clone()));

    let keycloak_issuer_url = env::var("KEYCLOAK_ISSUER_URL")
        .unwrap_or_else(|_| "http://localhost:8081/realms/notion-caldav-saas".to_string());
    let keycloak_client_id =
        env::var("KEYCLOAK_CLIENT_ID").unwrap_or_else(|_| "notion-caldav-saas-app".to_string());
    let keycloak_client_secret = env::var("KEYCLOAK_CLIENT_SECRET").ok();

    let oidc_client = session::build_oidc_client_with_startup_retry(
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

    let app_config = session::AppConfig {
        base_url: app_base_url,
    };

    let app = create_app(state, oidc_client, app_config).layer(session_layer);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("notion-ical-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
