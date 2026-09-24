use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::confirm_button::ConfirmButton;
use crate::page_shell::{CopyRow, HomeHeader, PageFooter};

#[derive(Clone, Serialize, Deserialize)]
pub struct NewCredential {
    pub username: String,
    pub password: String,
    pub mobileconfig_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CalendarCardData {
    pub public_id: String,
    pub label: String,
    pub caldav_url: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MePageData {
    pub email: String,
    pub billing_status_text: String,
    pub billing_cta_href: Option<String>,
    pub just_connected_a_calendar: bool,
    pub already_connected_names: Vec<String>,
    pub calendars: Vec<CalendarCardData>,
    pub account_caldav_username: String,
    pub account_caldav_password: Option<String>,
    pub mobileconfig_token: Option<String>,
    pub app_base_url: String,
}

const DASHBOARD_HEAD_STYLE: &str = r#"
.success-banner-gradient { background: linear-gradient(90deg, rgba(220, 252, 231, 0.5) 0%, rgba(220, 252, 231, 0.2) 100%); }
.error-banner-gradient { background: linear-gradient(90deg, rgba(254, 226, 226, 0.5) 0%, rgba(254, 226, 226, 0.2) 100%); }
"#;

#[cfg(feature = "ssr")]
const TRIAL_MONTHS: u32 = 6;
#[cfg(feature = "ssr")]
const FREE_DAILY_QUOTA: i64 = 10;

#[cfg(feature = "ssr")]
fn generate_token(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

#[cfg(feature = "ssr")]
fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use argon2::Argon2;
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

#[cfg(feature = "ssr")]
async fn resolve_user_id(
    pool: &sqlx::PgPool,
    claims: &axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims>,
) -> Result<i64, ServerFnError> {
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let (user_id, has_account_credential): (i64, bool) = sqlx::query_as(
        "INSERT INTO users (keycloak_sub, email) VALUES ($1, $2)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id, account_caldav_username IS NOT NULL AS has_account_credential",
    )
    .bind(sub)
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to resolve user: {e}")))?;

    if !has_account_credential {
        let username = format!("acct_{}", generate_token(12));
        let password = generate_token(24);
        let token = generate_token(32);
        if let Ok(hash) = hash_password(&password) {
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
    }

    Ok(user_id)
}

#[server]
async fn regenerate_account_credential() -> Result<NewCredential, ServerFnError> {
    let claims: axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims> =
        leptos_axum::extract().await?;
    let pool = use_context::<sqlx::PgPool>()
        .ok_or_else(|| ServerFnError::new("missing db pool context"))?;
    let user_id = resolve_user_id(&pool, &claims).await?;

    let new_username = format!("acct_{}", generate_token(12));
    let new_password = generate_token(24);
    let new_token = generate_token(32);
    let new_password_hash = hash_password(&new_password)
        .map_err(|e| ServerFnError::new(format!("failed to hash password: {e}")))?;

    sqlx::query(
        "UPDATE users SET account_caldav_username = $1, account_caldav_password_hash = $2, account_caldav_password = $3, mobileconfig_token = $4 WHERE id = $5",
    )
    .bind(&new_username)
    .bind(&new_password_hash)
    .bind(&new_password)
    .bind(&new_token)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to set account-level credential: {e}")))?;

    Ok(NewCredential {
        username: new_username,
        password: new_password,
        mobileconfig_token: new_token,
    })
}

#[server]
async fn load_me_data() -> Result<MePageData, ServerFnError> {
    use chrono::{DateTime, Months, Utc};

    let claims: axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims> =
        leptos_axum::extract().await?;
    let session: tower_sessions::Session = leptos_axum::extract().await?;
    let pool = use_context::<sqlx::PgPool>()
        .ok_or_else(|| ServerFnError::new("missing db pool context"))?;
    let app_base_url = use_context::<crate::page_shell::AppBaseUrl>()
        .ok_or_else(|| ServerFnError::new("missing app base url context"))?
        .0;
    let stripe_configured = use_context::<crate::page_shell::StripeConfigured>()
        .map(|v| v.0)
        .unwrap_or(false);

    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = resolve_user_id(&pool, &claims).await?;

    let (billing_status_text, billing_cta_href) = {
        let row: Option<(DateTime<Utc>, String)> =
            sqlx::query_as("SELECT trial_started_at, subscription_status FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        let (trial_started_at, subscription_status) =
            row.unwrap_or((Utc::now(), "none".to_string()));
        let paying = matches!(subscription_status.as_str(), "trialing" | "active");
        let lifetime_free = subscription_status == "lifetime_free";

        let (status_text, show_cta) = if paying {
            ("Subscribed — $1/year".to_string(), false)
        } else if lifetime_free {
            ("Lifetime access".to_string(), false)
        } else {
            let trial_end = trial_started_at
                .checked_add_months(Months::new(TRIAL_MONTHS))
                .unwrap_or(trial_started_at);
            if Utc::now() < trial_end {
                let free_until = trial_end.format("%Y-%m-%d").to_string();
                (format!("Free until {free_until}"), true)
            } else {
                let used_today: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sync_log sl
                     JOIN calendars c ON c.id = sl.calendar_id
                     WHERE c.user_id = $1 AND sl.source = 'webview' AND sl.action != 'delete'
                     AND sl.occurred_at >= date_trunc('day', now())",
                )
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap_or(0);
                (format!("{used_today}/{FREE_DAILY_QUOTA} events today"), true)
            }
        };
        let cta_href = (show_cta && stripe_configured).then(|| "/billing/checkout".to_string());
        (status_text, cta_href)
    };

    let calendars: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT public_id, display_name, caldav_username FROM calendars WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let (account_caldav_username, account_caldav_password, mobileconfig_token): (
        String,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT account_caldav_username, account_caldav_password, mobileconfig_token FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to load account caldav credential: {e}")))?;

    let just_connected_a_calendar: bool = {
        let stashed: Vec<(String, String, String)> = session
            .get("new_calendar_credentials")
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        if !stashed.is_empty() {
            let _ = session
                .remove::<Vec<(String, String, String)>>("new_calendar_credentials")
                .await;
        }
        !stashed.is_empty()
    };

    let already_connected_names: Vec<String> = {
        let stashed: Vec<String> = session
            .get("calendar_connect_errors")
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        if !stashed.is_empty() {
            let _ = session
                .remove::<Vec<String>>("calendar_connect_errors")
                .await;
        }
        stashed
    };
    let _ = session.save().await;

    let cards: Vec<CalendarCardData> = calendars
        .iter()
        .map(|(public_id, name, _caldav_username)| {
            let label = if name.is_empty() {
                public_id.clone()
            } else {
                name.clone()
            };
            CalendarCardData {
                public_id: public_id.clone(),
                label,
                caldav_url: format!("{app_base_url}/cal/{public_id}"),
            }
        })
        .collect();

    Ok(MePageData {
        email,
        billing_status_text,
        billing_cta_href,
        just_connected_a_calendar,
        already_connected_names,
        calendars: cards,
        account_caldav_username,
        account_caldav_password,
        mobileconfig_token,
        app_base_url,
    })
}

#[component]
pub fn MeRoutePage() -> impl IntoView {
    let data = Resource::new(|| (), |_| load_me_data());
    view! {
        <leptos_meta::Style>{DASHBOARD_HEAD_STYLE}</leptos_meta::Style>
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text="Your calendars — NotionCal"/>
        <Suspense fallback=|| ()>
            {move || data.get().and_then(|r| r.ok()).map(|data| view! {
                <MePage data=data/>
            })}
        </Suspense>
    }
}

#[component]
pub fn MePage(data: MePageData) -> impl IntoView {
    let has_calendars = !data.calendars.is_empty();
    let already_connected_names = data.already_connected_names.join(", ");
    let has_connect_errors = !data.already_connected_names.is_empty();

    let list = if data.calendars.is_empty() {
        view! {
            <div class="flex flex-col items-center justify-center py-xl text-center border border-dashed border-outline-variant rounded-lg">
                <span class="material-symbols-outlined !text-[48px] text-outline mb-md">"calendar_add_on"</span>
                <p class="text-body-lg text-on-surface-variant max-w-sm">"No calendars yet — connect Notion to get started."</p>
            </div>
        }
        .into_any()
    } else {
        data.calendars
            .into_iter()
            .map(|c| view! { <CalendarCard data=c/> })
            .collect_view()
            .into_any()
    };

    view! {
        <div id="me-root" class="pt-[64px]">
            <HomeHeader email=data.email/>
            <main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
                {data.just_connected_a_calendar.then(|| view! {
                    <div class="flex items-center gap-sm p-md success-banner-gradient border border-[#DCFCE7] rounded-lg" id="success-banner">
                        <div class="flex items-center justify-center w-6 h-6 bg-[#DCFCE7] text-[#166534] rounded-full shrink-0">
                            <span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">"check_circle"</span>
                        </div>
                        <p class="text-[#166534] font-medium text-body-md">"Calendar connected — use the account-wide CalDAV access below to subscribe."</p>
                    </div>
                })}
                {has_connect_errors.then(|| view! {
                    <div class="flex items-center gap-sm p-md error-banner-gradient border border-[#fecaca] rounded-lg">
                        <div class="flex items-center justify-center w-6 h-6 bg-[#fecaca] text-error rounded-full shrink-0">
                            <span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">"error"</span>
                        </div>
                        <p class="text-error font-medium text-body-md"><strong>{already_connected_names}</strong>" is already connected to your account — nothing changed."</p>
                    </div>
                })}
                <div class="flex items-center gap-sm p-md bg-surface-container-low border border-outline-variant rounded-lg">
                    <p class="text-on-surface-variant text-body-md">
                        {data.billing_status_text}
                        {data.billing_cta_href.map(|href| view! {
                            " "<a href=href class="text-secondary hover:underline font-medium">"Upgrade for $1/year"</a>
                        })}
                    </p>
                </div>
                <div class="flex flex-col md:flex-row md:items-end justify-between gap-md border-b border-outline-variant pb-md">
                    <div>
                        <h1 class="text-h1 font-semibold">"Your calendars"</h1>
                        <p class="text-on-surface-variant mt-1">"Manage and sync your Notion databases with your favorite calendar app."</p>
                    </div>
                    <a class="bg-surface border border-outline-variant text-primary px-md h-10 font-label-md text-label-md flex items-center justify-center gap-sm hover:border-outline transition-all active:scale-95" href="/connect/notion">
                        <span class="material-symbols-outlined !text-[20px]">"add"</span>
                        <span>"Connect another database"</span>
                    </a>
                </div>
                <AccountCredentialSection
                    account_caldav_username=data.account_caldav_username
                    account_caldav_password=data.account_caldav_password
                    mobileconfig_token=data.mobileconfig_token
                    app_base_url=data.app_base_url
                />
                {has_calendars.then(|| view! {
                    <p class="text-label-md text-on-surface-variant italic">
                        "Heads up: per-calendar CalDAV credentials below will be deprecated soon — use the account-wide access above instead."
                    </p>
                })}
                <div class="space-y-md">{list}</div>
                <PageFooter/>
            </main>
        </div>
    }
}

#[cfg(feature = "hydrate")]
fn spawn_client(fut: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}
#[cfg(not(feature = "hydrate"))]
fn spawn_client(_fut: impl std::future::Future<Output = ()> + 'static) {}

#[component]
fn RegenerateButton<F, Fut>(label: String, confirm_label: String, class: String, on_confirm: F) -> impl IntoView
where
    F: Fn() -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    use std::time::Duration;
    const CONFIRM_ARM_WINDOW: Duration = Duration::from_secs(3);
    let (confirming, set_confirming) = signal(false);
    let handle_click = move |_| {
        if confirming.get() {
            set_confirming.set(false);
            spawn_client(on_confirm());
        } else {
            set_confirming.set(true);
            set_timeout(move || set_confirming.set(false), CONFIRM_ARM_WINDOW);
        }
    };

    view! {
        <button type="button" class=class on:click=handle_click>
            {move || if confirming.get() { confirm_label.clone() } else { label.clone() }}
        </button>
    }
}

#[component]
fn AccountCredentialSection(
    account_caldav_username: String,
    account_caldav_password: Option<String>,
    mobileconfig_token: Option<String>,
    app_base_url: String,
) -> impl IntoView {
    let (credential, set_credential) = signal((
        account_caldav_username,
        account_caldav_password.zip(mobileconfig_token),
    ));

    let on_confirm = move || async move {
        if let Ok(cred) = regenerate_account_credential().await {
            set_credential.set((cred.username, Some((cred.password, cred.mobileconfig_token))));
        }
    };

    view! {
        <div class="bg-surface border border-outline-variant rounded-lg p-lg">
            <h2 class="font-semibold text-h3">"Account-wide CalDAV access"</h2>
            <p class="text-on-surface-variant text-body-md mt-1">"One username/password that gives a CalDAV client access to every calendar above at once — your calendar app will list them all automatically. Each calendar's own credentials above still work independently."</p>
            <CopyRow label="CalDAV URL" value=app_base_url/>
            {move || {
                let (username, password_and_token) = credential.get();
                match password_and_token {
                    Some((password, token)) => view! {
                        <CopyRow label="Username" value=username/>
                        <CopyRow label="CalDAV password" value=password/>
                        <a
                            class="inline-block mt-sm text-label-md text-secondary hover:underline"
                            href=format!("/me/account-caldav.mobileconfig?token={token}")
                            target="_blank"
                        >"Download for iOS (2-way sync)"</a>
                    }.into_any(),
                    None => view! {
                        <CopyRow label="Username" value=username/>
                        <p class="text-on-surface-variant text-body-md mt-1">"Password already set but no longer shown — click \"Regenerate\" below to see a new one."</p>
                    }.into_any(),
                }
            }}
            <div class="mt-md pt-md border-t border-outline-variant">
                <RegenerateButton
                    label="Regenerate".to_string()
                    confirm_label="Generate a new account-wide password? The old one will stop working immediately.".to_string()
                    class="text-label-md text-secondary hover:underline".to_string()
                    on_confirm=on_confirm
                />
            </div>
        </div>
    }
}

#[component]
fn CalendarCard(data: CalendarCardData) -> impl IntoView {
    let open_href = format!("/app/{}", data.public_id);
    let view_log_href = format!("/me/calendars/{}/log", data.public_id);
    let delete_action = format!("/me/calendars/{}/delete", data.public_id);

    view! {
        <div class="bg-surface border border-outline-variant rounded-lg p-lg hover:border-outline transition-colors duration-200">
            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-md mb-lg">
                <div class="flex items-center gap-sm">
                    <span class="text-h2">"🗓️"</span>
                    <h2 class="font-semibold text-h2">{data.label}</h2>
                    <span class="bg-[#DCFCE7] text-[#166534] px-xs py-[2px] rounded font-label-md text-[10px] uppercase tracking-wider">"Active"</span>
                </div>
                <a class="px-md h-8 border border-outline-variant hover:bg-surface-container-low font-label-md text-label-md transition-all flex items-center" href=open_href>"Open calendar"</a>
            </div>
            <CopyRow label="CalDAV URL" value=data.caldav_url/>
            <p class="text-on-surface-variant text-[13px] mt-sm">"Paste this link into Apple Calendar, Google Calendar, or any CalDAV app"</p>
            <div class="flex items-center gap-md mt-md pt-md border-t border-outline-variant">
                <a href=view_log_href class="text-label-md text-secondary hover:underline">"View sync log"</a>
                <div class="ml-auto">
                    <ConfirmButton
                        action=delete_action
                        label="Delete".to_string()
                        confirm_label="Delete this calendar? Your Notion data is untouched, but it will stop syncing.".to_string()
                        class="text-label-md text-error hover:underline".to_string()
                    />
                </div>
            </div>
        </div>
    }
}
