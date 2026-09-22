use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::confirm_button::ConfirmButton;

#[derive(Clone, Serialize, Deserialize)]
pub struct CalendarCardData {
    pub public_id: String,
    pub label: String,
    pub active_badge: String,
    pub open_calendar_label: String,
    pub url_row_html: String,
    pub username_row_html: String,
    pub password_row_html: String,
    pub paste_hint: String,
    pub regenerate_password_label: String,
    pub regenerate_confirm_label: String,
    pub regenerate_action: String,
    pub view_log_label: String,
    pub view_log_href: String,
    pub delete_label: String,
    pub delete_confirm_label: String,
    pub delete_action: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MePageData {
    pub html_lang: String,
    pub page_title: String,
    pub header_html: String,
    pub main_top_html: String,
    pub main_bottom_html: String,
    pub calendars: Vec<CalendarCardData>,
    pub empty_state_html: String,
}

const DASHBOARD_HEAD_STYLE: &str = r#"
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.success-banner-gradient { background: linear-gradient(90deg, rgba(220, 252, 231, 0.5) 0%, rgba(220, 252, 231, 0.2) 100%); }
.error-banner-gradient { background: linear-gradient(90deg, rgba(254, 226, 226, 0.5) 0%, rgba(254, 226, 226, 0.2) 100%); }
"#;

use crate::page_shell::GOOGLE_FONTS_HREF;

#[cfg(feature = "ssr")]
const TRIAL_MONTHS: u32 = 6;
#[cfg(feature = "ssr")]
const FREE_DAILY_QUOTA: i64 = 10;

#[cfg(feature = "ssr")]
fn copy_row(label: &str, value: &str) -> String {
    let escaped_value = crate::page_shell::html_escape(value);
    format!(
        r#"<div class="space-y-sm mt-sm">
<label class="font-label-md text-label-md text-on-surface-variant block uppercase tracking-wide">{label}</label>
<div class="flex gap-sm">
<input class="w-full h-10 px-md bg-surface-container-low border border-outline-variant font-code text-code focus:outline-none focus:ring-0 cursor-default" readonly type="text" value="{escaped_value}">
<button class="w-10 h-10 border border-outline-variant flex items-center justify-center hover:bg-surface-container-high transition-all active:bg-surface-container-highest shrink-0" onclick="copyToClipboard('{escaped_value}', this)">
<span class="material-symbols-outlined">content_copy</span>
</button>
</div>
</div>"#
    )
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
    let sub = claims.subject().as_str();

    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (keycloak_sub, email) VALUES ($1, $2)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id",
    )
    .bind(sub)
    .bind(&email)
    .fetch_one(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to upsert user: {e}")))?;

    let billing_card = {
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
        let cta_label = "Upgrade for $1/year";
        let cta = if show_cta && stripe_configured {
            format!(
                r#" <a href="/billing/checkout" class="text-secondary hover:underline font-medium">{cta_label}</a>"#
            )
        } else {
            String::new()
        };
        format!(
            r#"<div class="flex items-center gap-sm p-md bg-surface-container-low border border-outline-variant rounded-lg">
<p class="text-on-surface-variant text-body-md">{status_text}{cta}</p>
</div>"#
        )
    };

    let calendars: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT public_id, display_name, caldav_username FROM calendars WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let one_shot_plaintext_passwords: std::collections::HashMap<String, String> = {
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
        stashed
            .into_iter()
            .map(|(_, username, password)| (username, password))
            .collect()
    };

    let new_password_notice =
        "The CalDAV password below won't be shown again automatically — save or copy it now.";
    let banner = if !one_shot_plaintext_passwords.is_empty() {
        format!(
            r#"<div class="flex items-center gap-sm p-md success-banner-gradient border border-[#DCFCE7] rounded-lg" id="success-banner">
<div class="flex items-center justify-center w-6 h-6 bg-[#DCFCE7] text-[#166534] rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">check_circle</span>
</div>
<p class="text-[#166534] font-medium text-body-md">{new_password_notice}</p>
</div>"#
        )
    } else {
        String::new()
    };

    let connect_errors: Vec<String> = {
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
    let already_connected_suffix = "is already connected to your account — nothing changed.";
    let error_banner = if connect_errors.is_empty() {
        String::new()
    } else {
        let names = connect_errors
            .iter()
            .map(|n| crate::page_shell::html_escape(n))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"<div class="flex items-center gap-sm p-md error-banner-gradient border border-[#fecaca] rounded-lg">
<div class="flex items-center justify-center w-6 h-6 bg-[#fecaca] text-error rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">error</span>
</div>
<p class="text-error font-medium text-body-md"><strong>{names}</strong> {already_connected_suffix}</p>
</div>"#
        )
    };

    let (
        active_badge,
        open_calendar_label,
        caldav_password_label,
        caldav_url_label,
        username_label,
        paste_hint,
        regenerate_password_label,
        regenerate_confirm_label,
        view_log_label,
        delete_label,
        delete_confirm_label,
    ) = (
        "Active",
        "Open calendar",
        "CalDAV password",
        "CalDAV URL",
        "Username",
        "Paste this link into Apple Calendar, Google Calendar, or any CalDAV app",
        "Regenerate password",
        "Generate a new password? The old one will stop working immediately.",
        "View sync log",
        "Delete",
        "Delete this calendar? Your Notion data is untouched, but it will stop syncing.",
    );

    let cards: Vec<CalendarCardData> = calendars
        .iter()
        .map(|(public_id, name, caldav_username)| {
            let label = if name.is_empty() {
                public_id.as_str()
            } else {
                name.as_str()
            };
            let caldav_url = format!("{app_base_url}/cal/{public_id}");
            let password_row_html = match one_shot_plaintext_passwords.get(caldav_username) {
                Some(pw) => copy_row(caldav_password_label, pw),
                None => String::new(),
            };
            CalendarCardData {
                public_id: public_id.clone(),
                label: label.to_string(),
                active_badge: active_badge.to_string(),
                open_calendar_label: open_calendar_label.to_string(),
                url_row_html: copy_row(caldav_url_label, &caldav_url),
                username_row_html: copy_row(username_label, caldav_username),
                password_row_html,
                paste_hint: paste_hint.to_string(),
                regenerate_password_label: regenerate_password_label.to_string(),
                regenerate_confirm_label: regenerate_confirm_label.to_string(),
                regenerate_action: format!("/me/calendars/{public_id}/regenerate-password"),
                view_log_label: view_log_label.to_string(),
                view_log_href: format!("/me/calendars/{public_id}/log"),
                delete_label: delete_label.to_string(),
                delete_confirm_label: delete_confirm_label.to_string(),
                delete_action: format!("/me/calendars/{public_id}/delete"),
            }
        })
        .collect();

    let empty_state = "No calendars yet — connect Notion to get started.";
    let empty_state_html = format!(
        r#"<div class="flex flex-col items-center justify-center py-xl text-center border border-dashed border-outline-variant rounded-lg">
<span class="material-symbols-outlined !text-[48px] text-outline mb-md">calendar_add_on</span>
<p class="text-body-lg text-on-surface-variant max-w-sm">{empty_state}</p>
</div>"#
    );

    let header_html = crate::page_shell::top_nav_html(&email);

    let (heading, subheading, connect_more) = (
        "Your calendars",
        "Manage and sync your Notion databases with your favorite calendar app.",
        "Connect another database",
    );
    let main_top_html = format!(
        r#"{banner}
{error_banner}
{billing_card}
<div class="flex flex-col md:flex-row md:items-end justify-between gap-md border-b border-outline-variant pb-md">
<div>
<h1 class="text-h1 font-semibold">{heading}</h1>
<p class="text-on-surface-variant mt-1">{subheading}</p>
</div>
<a class="bg-surface border border-outline-variant text-primary px-md h-10 font-label-md text-label-md flex items-center justify-center gap-sm hover:border-outline transition-all active:scale-95" href="/connect/notion">
<span class="material-symbols-outlined">add</span>
<span>{connect_more}</span>
</a>
</div>"#
    );

    let main_bottom_html = r#"<p class="text-on-surface-variant text-[13px] pt-lg"><a class="underline hover:text-primary" href="/privacy">Privacy Policy</a> · <a class="underline hover:text-primary" href="/terms">Terms of Service</a></p>"#.to_string();

    let page_title = "Your calendars — NotionCal";

    Ok(MePageData {
        html_lang: "en".to_string(),
        page_title: page_title.to_string(),
        header_html,
        main_top_html,
        main_bottom_html,
        calendars: cards,
        empty_state_html,
    })
}

#[component]
pub fn MeRoutePage() -> impl IntoView {
    let data = Resource::new(|| (), |_| load_me_data());
    view! {
        <leptos_meta::Style>{DASHBOARD_HEAD_STYLE}</leptos_meta::Style>
        <leptos_meta::Link rel="stylesheet" href="/assets/style-auth-a.css"/>
        <leptos_meta::Link href=GOOGLE_FONTS_HREF rel="stylesheet"/>
        <leptos_meta::Script>{COPY_TO_CLIPBOARD_JS}</leptos_meta::Script>
        <Suspense fallback=|| ()>
            {move || data.get().and_then(|r| r.ok()).map(|data| view! {
                <leptos_meta::Html attr:lang=data.html_lang.clone()/>
                <leptos_meta::Title text=data.page_title.clone()/>
                <MePage data=data/>
            })}
        </Suspense>
    }
}

const COPY_TO_CLIPBOARD_JS: &str = r#"
function copyToClipboard(text, btn) {
  navigator.clipboard.writeText(text).then(() => {
    const icon = btn.querySelector('.material-symbols-outlined');
    const original = icon.innerText;
    icon.innerText = 'check';
    icon.classList.add('text-[#166534]');
    setTimeout(() => { icon.innerText = original; icon.classList.remove('text-[#166534]'); }, 2000);
  });
}
"#;

#[component]
pub fn MePage(data: MePageData) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    crate::page_shell::install_client_timezone_label();

    let header_html = data.header_html.clone();
    let main_top_html = data.main_top_html.clone();
    let main_bottom_html = data.main_bottom_html.clone();

    let list = if data.calendars.is_empty() {
        view! { <div inner_html=data.empty_state_html.clone()></div> }.into_any()
    } else {
        data.calendars
            .into_iter()
            .map(|c| view! { <CalendarCard data=c/> })
            .collect_view()
            .into_any()
    };

    view! {
        <div id="me-root">
            <div inner_html=header_html></div>
            <main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
                <div inner_html=main_top_html></div>
                <div class="space-y-md">{list}</div>
                <div inner_html=main_bottom_html></div>
            </main>
        </div>
    }
}

#[component]
fn CalendarCard(data: CalendarCardData) -> impl IntoView {
    let open_href = format!("/app/{}", data.public_id);

    view! {
        <div class="bg-surface border border-outline-variant rounded-lg p-lg hover:border-outline transition-colors duration-200">
            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-md mb-lg">
                <div class="flex items-center gap-sm">
                    <span class="text-h2">"🗓️"</span>
                    <h2 class="font-semibold text-h2">{data.label}</h2>
                    <span class="bg-[#DCFCE7] text-[#166534] px-xs py-[2px] rounded font-label-md text-[10px] uppercase tracking-wider">{data.active_badge}</span>
                </div>
                <a class="px-md h-8 border border-outline-variant hover:bg-surface-container-low font-label-md text-label-md transition-all flex items-center" href=open_href>{data.open_calendar_label}</a>
            </div>
            <div inner_html=data.url_row_html></div>
            <div inner_html=data.username_row_html></div>
            {(!data.password_row_html.is_empty()).then(|| view! { <div inner_html=data.password_row_html.clone()></div> })}
            <p class="text-on-surface-variant text-[13px] mt-sm">{data.paste_hint}</p>
            <div class="flex items-center gap-md mt-md pt-md border-t border-outline-variant">
                <ConfirmButton
                    action=data.regenerate_action
                    label=data.regenerate_password_label
                    confirm_label=data.regenerate_confirm_label
                    class="text-label-md text-secondary hover:underline".to_string()
                />
                <a href=data.view_log_href class="text-label-md text-secondary hover:underline">{data.view_log_label}</a>
                <div class="ml-auto">
                    <ConfirmButton
                        action=data.delete_action
                        label=data.delete_label
                        confirm_label=data.delete_confirm_label
                        class="text-label-md text-error hover:underline".to_string()
                    />
                </div>
            </div>
        </div>
    }
}
