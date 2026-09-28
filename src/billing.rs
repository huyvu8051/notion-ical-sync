use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use chrono::{DateTime, Months, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::{error, info, warn};

use crate::{session::find_or_create_user, AppState};

type HmacSha256 = Hmac<Sha256>;

pub const TRIAL_MONTHS: u32 = 6;
pub const FREE_DAILY_QUOTA: i64 = 10;

#[derive(sqlx::FromRow)]
struct BillingRow {
    trial_started_at: DateTime<Utc>,
    subscription_status: String,
}

pub enum AccessLevel {
    Unlimited,
    Quota { used_today: i64, limit: i64 },
}

pub fn trial_end(trial_started_at: DateTime<Utc>) -> DateTime<Utc> {
    trial_started_at
        .checked_add_months(Months::new(TRIAL_MONTHS))
        .unwrap_or(trial_started_at)
}

pub async fn effective_access(state: &AppState, user_id: i64) -> AccessLevel {
    let row: Option<BillingRow> =
        sqlx::query_as("SELECT trial_started_at, subscription_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    let Some(row) = row else {
        return AccessLevel::Unlimited;
    };

    let subscribed = matches!(
        row.subscription_status.as_str(),
        "trialing" | "active" | "lifetime_free"
    );
    if Utc::now() < trial_end(row.trial_started_at) || subscribed {
        return AccessLevel::Unlimited;
    }

    let used_today = count_writes_today(&state.db, user_id).await;
    AccessLevel::Quota {
        used_today,
        limit: FREE_DAILY_QUOTA,
    }
}

async fn count_writes_today(pool: &sqlx::PgPool, user_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log sl
         JOIN calendars c ON c.id = sl.calendar_id
         WHERE c.user_id = $1
           AND sl.source IN ('caldav', 'webview')
           AND sl.action IN ('create', 'update')
           AND sl.status = 'ok'
           AND sl.occurred_at >= date_trunc('day', now())",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

pub async fn enforce_quota(state: &AppState, user_id: i64) -> Result<(), ()> {
    match effective_access(state, user_id).await {
        AccessLevel::Unlimited => Ok(()),
        AccessLevel::Quota { used_today, limit } if used_today < limit => Ok(()),
        AccessLevel::Quota { .. } => Err(()),
    }
}

/// Paddle Billing (not Classic) config. Sandbox and live are entirely
/// separate datasets/API keys/base URLs — which one we're in is derived
/// from the API key's own prefix (`pdl_sdbx_...` vs `pdl_live_...`) rather
/// than a second env var, so it can't drift out of sync with the key.
#[derive(Clone)]
pub struct PaddleConfig {
    pub api_key: String,
    pub webhook_secret: String,
    pub price_id: String,
    pub client_token: String,
    pub api_base: &'static str,
}

impl PaddleConfig {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("PADDLE_API_KEY").ok()?;
        let webhook_secret = std::env::var("PADDLE_WEBHOOK_SECRET").ok()?;
        let price_id = std::env::var("PADDLE_PRICE_ID").ok()?;
        let client_token = std::env::var("PADDLE_CLIENT_TOKEN").ok()?;
        let api_base = if api_key.starts_with("pdl_sdbx_") {
            "https://sandbox-api.paddle.com"
        } else {
            "https://api.paddle.com"
        };
        Some(Self {
            api_key,
            webhook_secret,
            price_id,
            client_token,
            api_base,
        })
    }

    fn is_sandbox(&self) -> bool {
        self.api_base.contains("sandbox")
    }
}

fn verify_paddle_signature(secret: &str, header: &str, body: &[u8]) -> bool {
    let mut timestamp = None;
    let mut signature_hex = None;
    for part in header.split(';') {
        if let Some(t) = part.strip_prefix("ts=") {
            timestamp = Some(t);
        } else if let Some(h1) = part.strip_prefix("h1=") {
            signature_hex = Some(h1);
        }
    }
    let (Some(timestamp), Some(signature_hex)) = (timestamp, signature_hex) else {
        return false;
    };
    let Ok(expected) = hex_decode(signature_hex) else {
        return false;
    };
    // Paddle signs "{timestamp}:{raw_body}" (colon-joined), unlike Stripe's
    // "{timestamp}.{raw_body}".
    let signed_payload = [timestamp.as_bytes(), b":", body].concat();
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(&signed_payload);
    mac.verify_slice(&expected).is_ok()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if !s.len().is_multiple_of(2) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

pub async fn handle_paddle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(paddle) = state.paddle.as_ref() else {
        warn!("paddle webhook: PADDLE_* not configured, dropping event");
        return StatusCode::OK;
    };

    let signature = headers
        .get("paddle-signature")
        .and_then(|v| v.to_str().ok());
    let Some(signature) = signature else {
        warn!("paddle webhook: missing Paddle-Signature header");
        return StatusCode::BAD_REQUEST;
    };
    if !verify_paddle_signature(&paddle.webhook_secret, signature, &body) {
        warn!("paddle webhook: signature verification failed, dropping event");
        return StatusCode::BAD_REQUEST;
    }

    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "paddle webhook: invalid JSON body");
            return StatusCode::BAD_REQUEST;
        }
    };

    let event_type = json
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let data = json.get("data").cloned().unwrap_or_default();
    info!(event_type, "-> Paddle webhook event verified");

    match event_type {
        "transaction.completed" => {
            let user_id = data
                .pointer("/custom_data/user_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok());
            let customer_id = data.get("customer_id").and_then(|v| v.as_str());
            let subscription_id = data.get("subscription_id").and_then(|v| v.as_str());
            if let (Some(user_id), Some(customer_id), Some(subscription_id)) =
                (user_id, customer_id, subscription_id)
            {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET billing_customer_id = $1, billing_subscription_id = $2, subscription_status = 'trialing' WHERE id = $3",
                )
                .bind(customer_id)
                .bind(subscription_id)
                .bind(user_id)
                .execute(&state.db)
                .await
                {
                    warn!("paddle webhook: failed to link customer/subscription to user {}: {}", user_id, e);
                } else {
                    if let Err(e) =
                        defer_subscription_to_trial_end(&state, paddle, user_id, subscription_id)
                            .await
                    {
                        warn!(
                            "paddle webhook: failed to adjust trial billing date for subscription {}: {}",
                            subscription_id, e
                        );
                    }
                    if let Some(cfg) = state.email.clone() {
                        notify_user_by_id(&state, cfg, user_id, crate::email::subscribed_email).await;
                    }
                }
            } else {
                warn!("paddle webhook: transaction.completed missing custom_data.user_id/customer_id/subscription_id");
            }
        }
        "subscription.updated" => {
            let customer_id = data.get("customer_id").and_then(|v| v.as_str());
            let status = data.get("status").and_then(|v| v.as_str());
            if let (Some(customer_id), Some(status)) = (customer_id, status) {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET subscription_status = $1 WHERE billing_customer_id = $2",
                )
                .bind(status)
                .bind(customer_id)
                .execute(&state.db)
                .await
                {
                    warn!(
                        "paddle webhook: failed to update subscription_status for customer {}: {}",
                        customer_id, e
                    );
                }
            }
        }
        "subscription.canceled" => {
            let customer_id = data.get("customer_id").and_then(|v| v.as_str());
            let status = data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("canceled");
            if let Some(customer_id) = customer_id {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET subscription_status = $1 WHERE billing_customer_id = $2",
                )
                .bind(status)
                .bind(customer_id)
                .execute(&state.db)
                .await
                {
                    warn!(
                        "paddle webhook: failed to update subscription_status for customer {}: {}",
                        customer_id, e
                    );
                } else if let Some(cfg) = state.email.clone() {
                    notify_user_by_customer_id(
                        &state,
                        cfg,
                        customer_id,
                        crate::email::subscription_canceled_email,
                    )
                    .await;
                }
            }
        }
        "transaction.payment_failed" => {
            if let Some(customer_id) = data.get("customer_id").and_then(|v| v.as_str()) {
                if let Some(cfg) = state.email.clone() {
                    notify_user_by_customer_id(
                        &state,
                        cfg,
                        customer_id,
                        crate::email::payment_failed_email,
                    )
                    .await;
                }
            }
        }
        _ => {}
    }

    StatusCode::OK
}

/// Our trial length is dynamic (6 months from each user's own signup date),
/// but Paddle prices only support a fixed trial length for everyone. The
/// price is configured with a generous flat trial as an upper bound, and
/// every checkout immediately gets this follow-up call to push the actual
/// next charge to this user's real trial end (or to "now" if their trial
/// already ended before they checked out) — mirroring the old Stripe
/// `subscription_data[trial_end]` checkout param, which Paddle has no
/// equivalent for at transaction-creation time.
async fn defer_subscription_to_trial_end(
    state: &AppState,
    paddle: &PaddleConfig,
    user_id: i64,
    subscription_id: &str,
) -> Result<(), String> {
    let trial_started_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT trial_started_at FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    let Some(trial_started_at) = trial_started_at else {
        return Ok(());
    };
    let next_billed_at = trial_end(trial_started_at).max(Utc::now() + chrono::Duration::minutes(5));

    let url = format!("{}/subscriptions/{}", paddle.api_base, subscription_id);
    let body = serde_json::json!({
        "next_billed_at": next_billed_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "proration_billing_mode": "do_not_bill",
    });
    info!(subscription_id, "-> Paddle API request (defer next_billed_at to trial end)");
    let resp = state
        .client
        .patch(&url)
        .bearer_auth(&paddle.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Paddle subscription update failed ({status}): {text}"));
    }
    Ok(())
}

async fn notify_user_by_id(
    state: &AppState,
    cfg: crate::email::EmailConfig,
    user_id: i64,
    template: fn() -> (&'static str, String),
) {
    let row: Option<(String,)> = sqlx::query_as("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    if let Some((email,)) = row {
        let (subject, html) = template();
        crate::email::spawn_send(cfg, email, subject.to_string(), html);
    }
}

async fn notify_user_by_customer_id(
    state: &AppState,
    cfg: crate::email::EmailConfig,
    customer_id: &str,
    template: fn() -> (&'static str, String),
) {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT email FROM users WHERE billing_customer_id = $1")
            .bind(customer_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    if let Some((email,)) = row {
        let (subject, html) = template();
        crate::email::spawn_send(cfg, email, subject.to_string(), html);
    }
}

#[derive(sqlx::FromRow)]
struct CheckoutUserRow {
    #[allow(dead_code)]
    trial_started_at: DateTime<Utc>,
}

/// Paddle Billing checkout is opened client-side via Paddle.js (there's no
/// pure server-side "create a session, redirect to a hosted page" flow like
/// Stripe Checkout) — so this renders a tiny page that loads Paddle.js and
/// immediately opens the overlay, instead of returning a redirect.
pub async fn start_checkout(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    cfg: axum::Extension<crate::session::AppConfig>,
) -> impl IntoResponse {
    let Some(paddle) = state.paddle.as_ref() else {
        return crate::error_page::error_page(crate::error_page::OauthError::BillingNotConfigured);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email).await {
        Ok(id) => id,
        Err(_) => return crate::error_page::error_page(crate::error_page::OauthError::Generic),
    };

    let row: Option<CheckoutUserRow> =
        sqlx::query_as("SELECT trial_started_at FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    if row.is_none() {
        return crate::error_page::error_page(crate::error_page::OauthError::Generic);
    }

    let checkout_config = serde_json::json!({
        "items": [{ "priceId": paddle.price_id, "quantity": 1 }],
        "customer": { "email": email },
        "customData": { "user_id": user_id.to_string() },
        "settings": { "successUrl": format!("{}/me?checkout=success", cfg.base_url) },
    });
    let environment_js = if paddle.is_sandbox() {
        "Paddle.Environment.set(\"sandbox\");"
    } else {
        ""
    };

    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Opening checkout…</title>
<script src="https://cdn.paddle.com/paddle/v2/paddle.js"></script>
</head><body>
<p>Opening checkout…</p>
<script>
{environment_js}
Paddle.Initialize({{ token: {client_token} }});
Paddle.Checkout.open({checkout_config});
</script>
</body></html>"#,
        client_token = serde_json::to_string(&paddle.client_token).unwrap_or_default(),
        checkout_config = checkout_config,
    );

    Html(html).into_response()
}

pub async fn send_trial_reminders(state: &AppState) {
    let Some(cfg) = state.email.clone() else {
        return;
    };

    let rows: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, trial_started_at FROM users
         WHERE trial_reminder_sent_at IS NULL
           AND subscription_status NOT IN ('trialing', 'active')
           AND trial_started_at + interval '6 months' BETWEEN now() AND now() + interval '7 days'",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| {
        error!("failed to query users for trial reminders: {}", e);
        Vec::new()
    });

    for (user_id, email, trial_started_at) in rows {
        let free_until = trial_end(trial_started_at).format("%Y-%m-%d").to_string();
        let (subject, html) = crate::email::trial_ending_email(&free_until);
        crate::email::spawn_send(cfg.clone(), email, subject.to_string(), html);

        if let Err(e) = sqlx::query("UPDATE users SET trial_reminder_sent_at = now() WHERE id = $1")
            .bind(user_id)
            .execute(&state.db)
            .await
        {
            error!(
                "failed to stamp trial_reminder_sent_at for user {}: {}",
                user_id, e
            );
        }
    }
}

#[derive(Deserialize)]
pub struct ResetBillingRequest {
    email: String,
}

pub async fn reset_billing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResetBillingRequest>,
) -> impl IntoResponse {
    let Some(expected_secret) = state.admin_secret.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let provided = headers.get("X-Admin-Secret").and_then(|v| v.to_str().ok());
    if provided != Some(expected_secret) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: i64,
        billing_subscription_id: Option<String>,
    }
    let row: Option<UserRow> =
        sqlx::query_as("SELECT id, billing_subscription_id FROM users WHERE email = $1")
            .bind(&body.email)
            .fetch_optional(&state.db)
            .await
            .unwrap_or_else(|e| {
                error!("reset_billing: failed to look up user by email: {}", e);
                None
            });
    let Some(row) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut paddle_cancel_result = "no subscription on file";
    if let (Some(sub_id), Some(paddle)) =
        (row.billing_subscription_id.as_deref(), state.paddle.as_ref())
    {
        match cancel_paddle_subscription(&state, paddle, sub_id).await {
            Ok(()) => paddle_cancel_result = "cancelled",
            Err(e) => {
                warn!(
                    "reset_billing: failed to cancel Paddle subscription {}: {}",
                    sub_id, e
                );
                paddle_cancel_result = "cancel failed (see server logs) — DB reset anyway";
            }
        }
    }

    if let Err(e) = sqlx::query(
        "UPDATE users SET subscription_status = 'none', trial_started_at = now(),
         billing_customer_id = NULL, billing_subscription_id = NULL, trial_reminder_sent_at = NULL
         WHERE id = $1",
    )
    .bind(row.id)
    .execute(&state.db)
    .await
    {
        error!("reset_billing: failed to reset user {}: {}", row.id, e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to reset billing state",
        )
            .into_response();
    }

    info!(
        "reset_billing: reset user {} ({}) — paddle: {}",
        row.id, body.email, paddle_cancel_result
    );
    Json(serde_json::json!({
        "user_id": row.id,
        "paddle_subscription": paddle_cancel_result,
        "trial_started_at": "now (fresh 6-month trial)",
    }))
    .into_response()
}

async fn cancel_paddle_subscription(
    state: &AppState,
    paddle: &PaddleConfig,
    subscription_id: &str,
) -> Result<(), String> {
    let url = format!("{}/subscriptions/{}/cancel", paddle.api_base, subscription_id);
    info!(method = "POST", url = %url, "-> Paddle API request (cancel subscription)");
    let resp = state
        .client
        .post(&url)
        .bearer_auth(&paddle.api_key)
        .json(&serde_json::json!({ "effective_from": "immediately" }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Paddle cancel failed ({status}): {body}"));
    }
    Ok(())
}

#[derive(Deserialize, Default)]
pub struct GrantLifetimeRequest {
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    send_promo_email: bool,
    #[serde(default)]
    exclude_emails: Vec<String>,
}

pub async fn grant_lifetime_to_non_paying_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<GrantLifetimeRequest>>,
) -> impl IntoResponse {
    let Some(expected_secret) = state.admin_secret.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let provided = headers.get("X-Admin-Secret").and_then(|v| v.to_str().ok());
    if provided != Some(expected_secret) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let opts = body.map(|Json(b)| b).unwrap_or_default();

    #[derive(sqlx::FromRow)]
    struct NonPayingUserRow {
        id: i64,
        email: String,
        subscription_status: String,
        billing_subscription_id: Option<String>,
    }
    let rows: Vec<NonPayingUserRow> = sqlx::query_as(
        "SELECT id, email, subscription_status, billing_subscription_id FROM users WHERE subscription_status <> 'active'",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| {
        error!(
            "grant_lifetime_to_non_paying_users: failed to list non-paying users: {}",
            e
        );
        Vec::new()
    });
    let rows: Vec<NonPayingUserRow> = rows
        .into_iter()
        .filter(|r| !opts.exclude_emails.iter().any(|e| e.eq_ignore_ascii_case(&r.email)))
        .collect();

    if opts.dry_run {
        let preview: Vec<_> = rows
            .iter()
            .map(|r| serde_json::json!({ "id": r.id, "email": r.email, "subscription_status": r.subscription_status }))
            .collect();
        return Json(serde_json::json!({ "dry_run": true, "would_affect": preview.len(), "users": preview }))
            .into_response();
    }

    let mut paddle_subscriptions_cancelled = 0;
    if let Some(paddle) = state.paddle.as_ref() {
        for row in &rows {
            if let Some(sub_id) = row.billing_subscription_id.as_deref() {
                match cancel_paddle_subscription(&state, paddle, sub_id).await {
                    Ok(()) => paddle_subscriptions_cancelled += 1,
                    Err(e) => warn!(
                        "grant_lifetime_to_non_paying_users: failed to cancel Paddle subscription {} for user {}: {}",
                        sub_id, row.id, e
                    ),
                }
            }
        }
    }

    let granted_ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    match sqlx::query("UPDATE users SET subscription_status = 'lifetime_free' WHERE id = ANY($1)")
        .bind(&granted_ids)
        .execute(&state.db)
        .await
    {
        Ok(result) => {
            info!(
                "grant_lifetime_to_non_paying_users: granted lifetime_free to {} users, cancelled {} pending Paddle subscriptions",
                result.rows_affected(),
                paddle_subscriptions_cancelled
            );
            let mut promo_emails_sent = 0;
            if opts.send_promo_email {
                if let Some(cfg) = state.email.as_ref() {
                    let (subject, html) = crate::email::lifetime_promo_email();
                    for row in &rows {
                        crate::email::spawn_send(
                            cfg.clone(),
                            row.email.clone(),
                            subject.to_string(),
                            html.clone(),
                        );
                        promo_emails_sent += 1;
                    }
                }
            }
            Json(serde_json::json!({
                "users_granted": result.rows_affected(),
                "paddle_subscriptions_cancelled": paddle_subscriptions_cancelled,
                "promo_emails_sent": promo_emails_sent,
            }))
            .into_response()
        }
        Err(e) => {
            error!("grant_lifetime_to_non_paying_users: update failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to grant lifetime access",
            )
                .into_response()
        }
    }
}
