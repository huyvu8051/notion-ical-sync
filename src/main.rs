use std::{
    env,
    time::Duration,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use notion_ical_sync::{AppState, CaldavAllowWrites, create_app};

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

    let caldav_allow_writes = CaldavAllowWrites::from_env();
    let state = AppState::new(pool, caldav_allow_writes, webhook_secret);

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

    let app = create_app(state);

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("notion-ical-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
