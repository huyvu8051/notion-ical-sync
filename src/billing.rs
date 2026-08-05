use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use chrono::{DateTime, Months, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::{error, info, warn};

use crate::{auth::find_or_create_user, AppState};

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

/// Single source of truth for "can this user write right now" — reused by
/// both the quota-enforcement call sites and the /me dashboard card.
pub async fn effective_access(state: &AppState, user_id: i64) -> AccessLevel {
    let row: Option<BillingRow> =
        sqlx::query_as("SELECT trial_started_at, subscription_status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    // Fail-open: a DB hiccup mid-request must never lock a user out.
    let Some(row) = row else {
        return AccessLevel::Unlimited;
    };

    let subscribed = matches!(row.subscription_status.as_str(), "trialing" | "active");
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

/// The actual gate the 3 write paths (CalDAV PUT, webview create/update)
/// call before touching Notion.
pub async fn enforce_quota(state: &AppState, user_id: i64) -> Result<(), ()> {
    match effective_access(state, user_id).await {
        AccessLevel::Unlimited => Ok(()),
        AccessLevel::Quota { used_today, limit } if used_today < limit => Ok(()),
        AccessLevel::Quota { .. } => Err(()),
    }
}

#[derive(Clone)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    pub price_id: String,
}

impl StripeConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            secret_key: std::env::var("STRIPE_SECRET_KEY").ok()?,
            webhook_secret: std::env::var("STRIPE_WEBHOOK_SECRET").ok()?,
            price_id: std::env::var("STRIPE_PRICE_ID").ok()?,
        })
    }
}

#[derive(serde::Deserialize)]
struct CheckoutSessionResponse {
    url: String,
}

/// Creates a Stripe Checkout session in subscription mode and returns the
/// hosted checkout URL to redirect the user to.
///
/// `subscription_data[trial_end]` is set to the user's own trial_end (see
/// `trial_end()` above) so Stripe won't actually start charging until their
/// free 6 months are up, no matter when they check out — but Stripe rejects
/// any `trial_end` under 48h in the future, so right at the boundary (or
/// once the trial has already lapsed) we omit it and let Stripe bill
/// normally/immediately instead.
pub async fn create_checkout_session(
    state: &AppState,
    stripe: &StripeConfig,
    user_id: i64,
    email: &str,
    existing_customer_id: Option<&str>,
    trial_started_at: DateTime<Utc>,
    app_base_url: &str,
) -> Result<String, String> {
    let mut params: Vec<(&str, String)> = vec![
        ("mode", "subscription".to_string()),
        ("line_items[0][price]", stripe.price_id.clone()),
        ("line_items[0][quantity]", "1".to_string()),
        ("client_reference_id", user_id.to_string()),
        ("success_url", format!("{app_base_url}/me?checkout=success")),
        (
            "cancel_url",
            format!("{app_base_url}/me?checkout=cancelled"),
        ),
    ];
    match existing_customer_id {
        Some(customer_id) => params.push(("customer", customer_id.to_string())),
        None => params.push(("customer_email", email.to_string())),
    }
    let te = trial_end(trial_started_at);
    if te > Utc::now() + chrono::Duration::hours(48) {
        params.push(("subscription_data[trial_end]", te.timestamp().to_string()));
    }

    info!(user_id, "-> Stripe API request (create checkout session)");
    let resp = state
        .client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe.secret_key, Some(""))
        .form(&params)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Stripe checkout session creation failed ({status}): {body}"
        ));
    }

    resp.json::<CheckoutSessionResponse>()
        .await
        .map(|r| r.url)
        .map_err(|e| e.to_string())
}

/// Verifies `Stripe-Signature: t=<unix_ts>,v1=<hex>[,v1=<hex>...]` — HMAC-SHA256
/// over the literal string "{t}.{raw_body}" (not the raw body alone, unlike
/// Notion's own webhook scheme in webhook.rs). Multiple v1= values can appear
/// during Stripe's signing-secret rotation; accept if any one matches.
fn verify_stripe_signature(secret: &str, header: &str, body: &[u8]) -> bool {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        if let Some(t) = part.strip_prefix("t=") {
            timestamp = Some(t);
        } else if let Some(v1) = part.strip_prefix("v1=") {
            signatures.push(v1);
        }
    }
    let Some(timestamp) = timestamp else {
        return false;
    };
    if signatures.is_empty() {
        return false;
    }

    let signed_payload = [timestamp.as_bytes(), b".", body].concat();
    signatures.iter().any(|sig_hex| {
        let Ok(expected) = hex_decode(sig_hex) else {
            return false;
        };
        let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(&signed_payload);
        mac.verify_slice(&expected).is_ok()
    })
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

/// Handles Stripe's webhook events for the $1/year subscription: links a
/// Stripe customer/subscription to our user on checkout completion, then
/// keeps `subscription_status` in sync as Stripe's own status changes.
pub async fn handle_stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(stripe) = state.stripe.as_ref() else {
        warn!("stripe webhook: STRIPE_* not configured, dropping event");
        return StatusCode::OK;
    };

    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok());
    let Some(signature) = signature else {
        warn!("stripe webhook: missing Stripe-Signature header");
        return StatusCode::BAD_REQUEST;
    };
    if !verify_stripe_signature(&stripe.webhook_secret, signature, &body) {
        warn!("stripe webhook: signature verification failed, dropping event");
        return StatusCode::BAD_REQUEST;
    }

    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "stripe webhook: invalid JSON body");
            return StatusCode::BAD_REQUEST;
        }
    };

    let event_type = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let data = json.pointer("/data/object").cloned().unwrap_or_default();
    info!(event_type, "-> Stripe webhook event verified");

    match event_type {
        "checkout.session.completed" => {
            let user_id = data
                .get("client_reference_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok());
            let customer_id = data.get("customer").and_then(|v| v.as_str());
            let subscription_id = data.get("subscription").and_then(|v| v.as_str());
            if let (Some(user_id), Some(customer_id), Some(subscription_id)) =
                (user_id, customer_id, subscription_id)
            {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET stripe_customer_id = $1, stripe_subscription_id = $2, subscription_status = 'trialing' WHERE id = $3",
                )
                .bind(customer_id)
                .bind(subscription_id)
                .bind(user_id)
                .execute(&state.db)
                .await
                {
                    warn!("stripe webhook: failed to link customer/subscription to user {}: {}", user_id, e);
                } else if let Some(cfg) = state.email.clone() {
                    // This — not `customer.subscription.updated` — is the one genuine
                    // "just subscribed" moment; `.updated` fires on lots of unrelated
                    // changes too and would spam a confirmation email repeatedly.
                    notify_user_by_id(&state, cfg, user_id, crate::email::subscribed_email).await;
                }
            } else {
                warn!("stripe webhook: checkout.session.completed missing client_reference_id/customer/subscription");
            }
        }
        "customer.subscription.updated" => {
            let customer_id = data.get("customer").and_then(|v| v.as_str());
            let status = data.get("status").and_then(|v| v.as_str());
            if let (Some(customer_id), Some(status)) = (customer_id, status) {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET subscription_status = $1 WHERE stripe_customer_id = $2",
                )
                .bind(status)
                .bind(customer_id)
                .execute(&state.db)
                .await
                {
                    warn!(
                        "stripe webhook: failed to update subscription_status for customer {}: {}",
                        customer_id, e
                    );
                }
            }
        }
        "customer.subscription.deleted" => {
            let customer_id = data.get("customer").and_then(|v| v.as_str());
            let status = data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("canceled");
            if let Some(customer_id) = customer_id {
                if let Err(e) = sqlx::query(
                    "UPDATE users SET subscription_status = $1 WHERE stripe_customer_id = $2",
                )
                .bind(status)
                .bind(customer_id)
                .execute(&state.db)
                .await
                {
                    warn!(
                        "stripe webhook: failed to update subscription_status for customer {}: {}",
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
        "invoice.payment_failed" => {
            if let Some(customer_id) = data.get("customer").and_then(|v| v.as_str()) {
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

/// Shared by the webhook arms above that need to email a user identified by
/// our own `users.id` (checkout completion, where we already have it from
/// `client_reference_id`).
async fn notify_user_by_id(
    state: &AppState,
    cfg: crate::email::EmailConfig,
    user_id: i64,
    template: fn(crate::i18n::Lang) -> (&'static str, String),
) {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT email, preferred_lang FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    if let Some((email, lang_code)) = row {
        let (subject, html) = template(crate::i18n::Lang::from_code(&lang_code));
        crate::email::spawn_send(cfg, email, subject.to_string(), html);
    }
}

/// Shared by the webhook arms above that only carry a Stripe customer id
/// (subscription/invoice events), looked up back to our own user row.
async fn notify_user_by_customer_id(
    state: &AppState,
    cfg: crate::email::EmailConfig,
    customer_id: &str,
    template: fn(crate::i18n::Lang) -> (&'static str, String),
) {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT email, preferred_lang FROM users WHERE stripe_customer_id = $1")
            .bind(customer_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    if let Some((email, lang_code)) = row {
        let (subject, html) = template(crate::i18n::Lang::from_code(&lang_code));
        crate::email::spawn_send(cfg, email, subject.to_string(), html);
    }
}

#[derive(sqlx::FromRow)]
struct CheckoutUserRow {
    trial_started_at: DateTime<Utc>,
    stripe_customer_id: Option<String>,
}

/// `GET /billing/checkout` — OIDC-gated. Redirects to a Stripe-hosted
/// checkout session for the logged-in user's $1/year subscription.
pub async fn start_checkout(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    cfg: axum::Extension<crate::auth::AppConfig>,
    lang: crate::i18n::Lang,
) -> impl IntoResponse {
    let Some(stripe) = state.stripe.as_ref() else {
        return crate::oauth::error_page(lang, crate::oauth::OauthError::BillingNotConfigured);
    };

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => return crate::oauth::error_page(lang, crate::oauth::OauthError::Generic),
    };

    let row: Option<CheckoutUserRow> =
        sqlx::query_as("SELECT trial_started_at, stripe_customer_id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some(row) = row else {
        return crate::oauth::error_page(lang, crate::oauth::OauthError::Generic);
    };

    match create_checkout_session(
        &state,
        stripe,
        user_id,
        &email,
        row.stripe_customer_id.as_deref(),
        row.trial_started_at,
        &cfg.base_url,
    )
    .await
    {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(e) => {
            warn!(
                "failed to create stripe checkout session for user {}: {}",
                user_id, e
            );
            crate::oauth::error_page(
                lang,
                crate::oauth::OauthError::FailedToCreateCheckoutSession,
            )
        }
    }
}

/// Daily background job (see `main.rs`'s spawned interval loop, modeled on
/// the existing 10-minute Notion refresh) — emails anyone whose free 6
/// months end within the next 7 days and who hasn't subscribed, then stamps
/// `trial_reminder_sent_at` so a later run of this same job never resends it.
pub async fn send_trial_reminders(state: &AppState) {
    let Some(cfg) = state.email.clone() else {
        return;
    };

    let rows: Vec<(i64, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, preferred_lang, trial_started_at FROM users
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

    for (user_id, email, lang_code, trial_started_at) in rows {
        let free_until = trial_end(trial_started_at).format("%Y-%m-%d").to_string();
        let (subject, html) =
            crate::email::trial_ending_email(crate::i18n::Lang::from_code(&lang_code), &free_until);
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

/// Dev/test-only: resets one user's trial/subscription state back to a
/// fresh 6-month trial, cancelling any live Stripe subscription first so
/// Stripe and our DB don't end up disagreeing. Gated by `X-Admin-Secret`
/// matching `state.admin_secret` — `None` (unset `ADMIN_SECRET`) disables
/// the route entirely rather than leaving a real reset action reachable
/// with no gate. Never meant to be user-facing: there's no reason a real
/// customer should ever be able to rewind their own trial clock.
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
        stripe_subscription_id: Option<String>,
    }
    let row: Option<UserRow> =
        sqlx::query_as("SELECT id, stripe_subscription_id FROM users WHERE email = $1")
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

    let mut stripe_cancel_result = "no subscription on file";
    if let (Some(sub_id), Some(stripe)) =
        (row.stripe_subscription_id.as_deref(), state.stripe.as_ref())
    {
        match cancel_stripe_subscription(&state, stripe, sub_id).await {
            Ok(()) => stripe_cancel_result = "cancelled",
            Err(e) => {
                // Not fatal — the subscription may already be canceled on
                // Stripe's side (e.g. a prior manual cancel), which is fine;
                // still proceed to reset the DB row either way so the test
                // account is usable again.
                warn!(
                    "reset_billing: failed to cancel Stripe subscription {}: {}",
                    sub_id, e
                );
                stripe_cancel_result = "cancel failed (see server logs) — DB reset anyway";
            }
        }
    }

    if let Err(e) = sqlx::query(
        "UPDATE users SET subscription_status = 'none', trial_started_at = now(),
         stripe_customer_id = NULL, stripe_subscription_id = NULL, trial_reminder_sent_at = NULL
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
        "reset_billing: reset user {} ({}) — stripe: {}",
        row.id, body.email, stripe_cancel_result
    );
    Json(serde_json::json!({
        "user_id": row.id,
        "stripe_subscription": stripe_cancel_result,
        "trial_started_at": "now (fresh 6-month trial)",
    }))
    .into_response()
}

async fn cancel_stripe_subscription(
    state: &AppState,
    stripe: &StripeConfig,
    subscription_id: &str,
) -> Result<(), String> {
    let url = format!("https://api.stripe.com/v1/subscriptions/{subscription_id}");
    info!(notion_method = "DELETE", notion_url = %url, "-> Stripe API request (cancel subscription)");
    let resp = state
        .client
        .delete(&url)
        .basic_auth(&stripe.secret_key, Some(""))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Stripe cancel failed ({status}): {body}"));
    }
    Ok(())
}
