//! Keycloak/OIDC wiring for the SaaS's own login (separate from the Notion
//! OAuth in oauth.rs, which is *this app's* permission to read/write a
//! user's Notion workspace — two different identities, don't conflate them).
//! Pattern copied from biolink-vn/src/auth.rs, which has the same axum-oidc +
//! tower-sessions shape.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect};
use axum_oidc::openidconnect::core::CoreGenderClaim;
use axum_oidc::openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use axum_oidc::{
    EmptyAdditionalClaims, OidcClaims, OidcClient, OidcRpInitiatedLogout, OidcSession,
};
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

/// Keycloak and this app land on the same node, so a node-level event
/// (host reboot, containerd restart) that recreates every pod's sandbox at
/// once routinely leaves Keycloak still coming back up when this process
/// starts — a real incident on 2026-08-07 crash-looped the whole app for
/// about a minute because a single transient 503 from Keycloak's discovery
/// endpoint was treated as fatal. Retry with backoff instead of panicking on
/// the first failure; only give up once Keycloak has had a real chance
/// (~2 minutes) to come back.
const OIDC_DISCOVERY_MAX_ATTEMPTS: u32 = 8;
const OIDC_DISCOVERY_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);
const OIDC_DISCOVERY_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn build_oidc_client(
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

        let issuer_url =
            IssuerUrl::new(issuer.clone()).expect("invalid KEYCLOAK_ISSUER_URL");
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

/// Find-or-create the `users` row for this Keycloak login, returning its id.
/// Also the single choke point for the welcome email: `(xmax = 0)` tells us
/// whether this INSERT actually inserted a new row vs. hit the ON CONFLICT
/// branch, so every one of this function's callers gets welcome-email
/// coverage for free instead of needing their own "is this a new user" check.
pub async fn find_or_create_user(
    state: &AppState,
    keycloak_sub: &str,
    email: &str,
    lang: crate::i18n::Lang,
) -> Result<i64, sqlx::Error> {
    let (id, inserted): (i64, bool) = sqlx::query_as(
        "INSERT INTO users (keycloak_sub, email, preferred_lang) VALUES ($1, $2, $3)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id, (xmax = 0) AS inserted",
    )
    .bind(keycloak_sub)
    .bind(email)
    .bind(lang.code())
    .fetch_one(&state.db)
    .await?;

    if inserted {
        if let Some(cfg) = state.email.clone() {
            let (subject, html) = crate::email::welcome_email(lang);
            crate::email::spawn_send(cfg, email.to_string(), subject.to_string(), html);
        }
    }

    Ok(id)
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) const AUTH_STYLE: &str = r#"
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, sans-serif; max-width: 480px; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.5; }
  .top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 2rem; }
  .top-nav a.logout { font-size: 0.85rem; color: #666; text-decoration: none; }
  .cal-list { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 0.75rem; }
  .cal-card { display: block; padding: 0.9rem 1rem; background: #f6f6f6; border-radius: 12px; }
  .cal-card-title { font-weight: 600; margin-bottom: 0.4rem; }
  .cal-card a { color: #2563eb; text-decoration: none; }
  .hint { color: #666; font-size: 0.9rem; }
  .header-row { display: flex; justify-content: space-between; align-items: center; gap: 1rem; margin-bottom: 1.5rem; }
  .header-row h1 { margin: 0; }
  .connect-btn, .connect-btn-secondary { display: inline-block; padding: 0.55rem 1.1rem; border-radius: 8px; text-decoration: none; font-size: 0.9rem; cursor: pointer; border: none; font-family: inherit; }
  .connect-btn { background: #171717; color: #fff; margin-top: 1rem; }
  .connect-btn-secondary { border: 1px solid #ddd; color: #171717; background: #fff; white-space: nowrap; }
  .cred-row { font-size: 0.85rem; color: #444; margin: 0.15rem 0; }
  .cred-label { color: #888; margin-right: 0.35rem; }
  .banner-success { background: #dcfce7; color: #166534; padding: 0.75rem 1rem; border-radius: 8px; margin-bottom: 1.25rem; font-size: 0.9rem; }
  .banner-error { background: #fee2e2; color: #991b1b; padding: 0.75rem 1rem; border-radius: 8px; margin-bottom: 1.25rem; font-size: 0.9rem; }
  code { font-family: ui-monospace, monospace; background: #eee; padding: 0.1rem 0.35rem; border-radius: 4px; font-size: 0.85rem; }
  .connect-card { margin-top: 2rem; }
  .reassure-list { list-style: none; padding: 0; margin-top: 1.5rem; font-size: 0.85rem; color: #555; }
  .reassure-list li { margin: 0.35rem 0; }
  .reassure-list li::before { content: "✓ "; color: #16a34a; }
  .db-list { display: flex; flex-direction: column; gap: 0.6rem; margin: 1.25rem 0; }
  .db-card { display: flex; align-items: center; gap: 0.6rem; padding: 0.75rem 1rem; border: 1px solid #e5e5e5; border-radius: 8px; cursor: pointer; font-size: 0.95rem; }
  .db-card-disabled { opacity: 0.5; cursor: not-allowed; }
  .db-name { font-weight: 500; }
  .db-meta { color: #888; font-size: 0.8rem; margin-left: auto; }
  .db-warning { color: #ba1a1a; font-size: 0.8rem; margin-left: auto; }
  .action-bar { display: flex; justify-content: space-between; margin-top: 1.5rem; }
</style>
"#;

/// Post-login landing: lists the user's own calendars, with a CTA to connect
/// more Notion databases (see oauth.rs). Doubles as the "onboarding
/// complete" screen right after `create_calendars` redirects here.
/// `/me`'s copy in both languages — a plain struct of `&'static str` fields
/// rather than a translation-string crate, since this is the only page with
/// this much dynamic-content-interleaved-with-copy; see i18n.rs for the
/// detection/toggle machinery this plugs into.
struct MeLabels {
    html_lang: &'static str,
    error_generic: &'static str,
    new_password_notice: &'static str,
    already_connected_suffix: &'static str,
    caldav_password_label: &'static str,
    caldav_url_label: &'static str,
    username_label: &'static str,
    active_badge: &'static str,
    open_calendar: &'static str,
    paste_hint: &'static str,
    reveal_password: &'static str,
    regenerate_password: &'static str,
    regenerate_confirm: &'static str,
    view_log: &'static str,
    delete: &'static str,
    delete_confirm: &'static str,
    empty_state: &'static str,
    page_title: &'static str,
    logout_title: &'static str,
    heading: &'static str,
    subheading: &'static str,
    connect_more: &'static str,
    billing_free_until: &'static str,
    billing_subscribed: &'static str,
    billing_quota: &'static str,
    billing_upgrade_cta: &'static str,
}

const ME_LABELS_VI: MeLabels = MeLabels {
    html_lang: "vi",
    error_generic: "Có lỗi xảy ra.",
    new_password_notice:
        "Mật khẩu CalDAV bên dưới sẽ không tự động hiển thị lại — lưu lại hoặc copy ngay.",
    already_connected_suffix: "đã được kết nối trong tài khoản của bạn rồi — không có gì thay đổi.",
    caldav_password_label: "Mật khẩu CalDAV",
    caldav_url_label: "CalDAV URL",
    username_label: "Username",
    active_badge: "Đang hoạt động",
    open_calendar: "Mở lịch",
    paste_hint: "Dán link này vào Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào",
    reveal_password: "Hiện mật khẩu",
    regenerate_password: "Tạo lại mật khẩu",
    regenerate_confirm: "Tạo mật khẩu mới? Mật khẩu cũ sẽ ngừng hoạt động ngay.",
    view_log: "Xem log đồng bộ",
    delete: "Xoá",
    delete_confirm:
        "Xoá calendar này? Dữ liệu trên Notion không bị ảnh hưởng, nhưng lịch sẽ ngừng đồng bộ.",
    empty_state: "Chưa có calendar nào — kết nối Notion để bắt đầu.",
    page_title: "Trang của bạn — NotionCal",
    logout_title: "Đăng xuất",
    heading: "Calendar của bạn",
    subheading:
        "Quản lý và đồng bộ hóa các cơ sở dữ liệu Notion với ứng dụng lịch yêu thích của bạn.",
    connect_more: "Kết nối thêm cơ sở dữ liệu",
    billing_free_until: "Miễn phí đến {date}",
    billing_subscribed: "Đã đăng ký — $1/năm",
    billing_quota: "{used}/{limit} sự kiện hôm nay",
    billing_upgrade_cta: "Nâng cấp $1/năm",
};

const ME_LABELS_EN: MeLabels = MeLabels {
    html_lang: "en",
    error_generic: "Something went wrong.",
    new_password_notice:
        "The CalDAV password below won't be shown again automatically — save or copy it now.",
    already_connected_suffix: "is already connected to your account — nothing changed.",
    caldav_password_label: "CalDAV password",
    caldav_url_label: "CalDAV URL",
    username_label: "Username",
    active_badge: "Active",
    open_calendar: "Open calendar",
    paste_hint: "Paste this link into Apple Calendar, Google Calendar, or any CalDAV app",
    reveal_password: "Show password",
    regenerate_password: "Regenerate password",
    regenerate_confirm: "Generate a new password? The old one will stop working immediately.",
    view_log: "View sync log",
    delete: "Delete",
    delete_confirm:
        "Delete this calendar? Your Notion data is untouched, but it will stop syncing.",
    empty_state: "No calendars yet — connect Notion to get started.",
    page_title: "Your calendars — NotionCal",
    logout_title: "Log out",
    heading: "Your calendars",
    subheading: "Manage and sync your Notion databases with your favorite calendar app.",
    connect_more: "Connect another database",
    billing_free_until: "Free until {date}",
    billing_subscribed: "Subscribed — $1/year",
    billing_quota: "{used}/{limit} events today",
    billing_upgrade_cta: "Upgrade for $1/year",
};

pub async fn me(
    claims: OidcClaims<EmptyAdditionalClaims>,
    State(state): State<AppState>,
    session: tower_sessions::Session,
    cfg: axum::Extension<AppConfig>,
    lang: crate::i18n::Lang,
    request: axum::extract::Request,
) -> axum::response::Response {
    let l = match lang {
        crate::i18n::Lang::En => &ME_LABELS_EN,
        crate::i18n::Lang::Vi => &ME_LABELS_VI,
    };
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();

    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                l.error_generic,
            )
                .into_response()
        }
    };

    let billing_card = {
        let row: Option<(chrono::DateTime<chrono::Utc>, String)> =
            sqlx::query_as("SELECT trial_started_at, subscription_status FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let (trial_started_at, subscription_status) =
            row.unwrap_or((chrono::Utc::now(), "none".to_string()));
        let subscribed = matches!(subscription_status.as_str(), "trialing" | "active");

        let (status_text, show_cta) = if subscribed {
            (l.billing_subscribed.to_string(), false)
        } else {
            match crate::billing::effective_access(&state, user_id).await {
                crate::billing::AccessLevel::Unlimited => {
                    let free_until = crate::billing::trial_end(trial_started_at)
                        .format("%Y-%m-%d")
                        .to_string();
                    (l.billing_free_until.replace("{date}", &free_until), true)
                }
                crate::billing::AccessLevel::Quota { used_today, limit } => (
                    l.billing_quota
                        .replace("{used}", &used_today.to_string())
                        .replace("{limit}", &limit.to_string()),
                    true,
                ),
            }
        };
        let cta = if show_cta && state.stripe.is_some() {
            format!(
                r#" <a href="/billing/checkout" class="text-secondary hover:underline font-medium">{}</a>"#,
                l.billing_upgrade_cta
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
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Plaintext CalDAV passwords only ever exist for one request — a
    // calendar just created, or one whose password was just revealed or
    // regenerated (see oauth.rs), stashes it here in the session for this
    // one render.
    let new_passwords = crate::oauth::take_new_calendar_credentials(&session).await;
    let banner = if !new_passwords.is_empty() {
        format!(
            r#"<div class="flex items-center gap-sm p-md success-banner-gradient border border-[#DCFCE7] rounded-lg" id="success-banner">
<div class="flex items-center justify-center w-6 h-6 bg-[#DCFCE7] text-[#166534] rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">check_circle</span>
</div>
<p class="text-[#166534] font-medium text-body-md">{}</p>
</div>"#,
            l.new_password_notice
        )
    } else {
        String::new()
    };

    // A user can't add the exact same database to their own account twice
    // (UNIQUE(user_id, database_id), see migrations/0003) — surface that
    // here instead of failing silently. Different users *can* now each have
    // their own subscription to the same Notion database.
    let connect_errors = crate::oauth::take_calendar_connect_errors(&session).await;
    let error_banner = if connect_errors.is_empty() {
        String::new()
    } else {
        let names = connect_errors
            .iter()
            .map(|n| html_escape(n))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"<div class="flex items-center gap-sm p-md error-banner-gradient border border-[#fecaca] rounded-lg">
<div class="flex items-center justify-center w-6 h-6 bg-[#fecaca] text-error rounded-full shrink-0">
<span class="material-symbols-outlined !text-[16px]" style="font-variation-settings: 'FILL' 1;">error</span>
</div>
<p class="text-error font-medium text-body-md"><strong>{names}</strong> {suffix}</p>
</div>"#,
            suffix = l.already_connected_suffix
        )
    };

    fn copy_row(label: &str, value: &str) -> String {
        let escaped_value = html_escape(value);
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

    let cards: Vec<app::me::CalendarCardData> = calendars
        .iter()
        .map(|(public_id, name, caldav_username)| {
            let label = if name.is_empty() { public_id.as_str() } else { name.as_str() };
            let caldav_url = format!("{}/cal/{}", cfg.base_url, public_id);
            let password_row_html = match new_passwords.get(caldav_username) {
                Some(pw) => copy_row(l.caldav_password_label, pw),
                None => String::new(),
            };
            app::me::CalendarCardData {
                public_id: public_id.clone(),
                label: label.to_string(),
                active_badge: l.active_badge.to_string(),
                open_calendar_label: l.open_calendar.to_string(),
                url_row_html: copy_row(l.caldav_url_label, &caldav_url),
                username_row_html: copy_row(l.username_label, caldav_username),
                password_row_html,
                paste_hint: l.paste_hint.to_string(),
                reveal_password_label: l.reveal_password.to_string(),
                reveal_password_action: format!("/me/calendars/{public_id}/reveal-password"),
                regenerate_password_label: l.regenerate_password.to_string(),
                regenerate_confirm_label: l.regenerate_confirm.to_string(),
                regenerate_action: format!("/me/calendars/{public_id}/regenerate-password"),
                view_log_label: l.view_log.to_string(),
                view_log_href: format!("/me/calendars/{public_id}/log"),
                delete_label: l.delete.to_string(),
                delete_confirm_label: l.delete_confirm.to_string(),
                delete_action: format!("/me/calendars/{public_id}/delete"),
            }
        })
        .collect();

    let empty_state_html = format!(
        r#"<div class="flex flex-col items-center justify-center py-xl text-center border border-dashed border-outline-variant rounded-lg">
<span class="material-symbols-outlined !text-[48px] text-outline mb-md">calendar_add_on</span>
<p class="text-body-lg text-on-surface-variant max-w-sm">{}</p>
</div>"#,
        l.empty_state
    );

    let lang_toggle = crate::i18n::lang_toggle(lang, "/me");

    // Fully self-contained (all tags balanced) — see module doc in
    // crates/app/src/me.rs for why: inner_html content is pushed straight
    // into the SSR HTML stream, not parsed in an isolated fragment context,
    // so an unclosed tag here would silently reshape the rest of the
    // document instead of being contained. `<main>` is a real view!
    // element in crates/app, not part of either string below.
    let header_html = format!(
        r#"<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
<div class="flex justify-between items-center h-16 px-lg w-full max-w-[1280px] mx-auto">
<span class="text-h1 font-semibold tracking-tighter text-primary">NotionCal</span>
<div class="flex items-center space-x-md">
{lang_toggle}
<span class="text-on-surface-variant font-label-md text-label-md">{email}</span>
<a class="flex items-center justify-center w-8 h-8 hover:bg-surface-container-low transition-colors duration-200 rounded" href="/logout" title="{logout_title}">
<span class="material-symbols-outlined">logout</span>
</a>
</div>
</div>
</header>"#,
        lang_toggle = lang_toggle,
        email = html_escape(&email),
        logout_title = l.logout_title,
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
</div>"#,
        heading = l.heading,
        subheading = l.subheading,
        connect_more = l.connect_more,
    );

    let main_bottom_html = r#"<p class="text-on-surface-variant text-[13px] pt-lg"><a class="underline hover:text-primary" href="/privacy">Privacy Policy</a> · <a class="underline hover:text-primary" href="/terms">Terms of Service</a></p>"#
        .to_string();

    let data = app::me::MePageData {
        html_lang: l.html_lang.to_string(),
        page_title: l.page_title.to_string(),
        header_html,
        main_top_html,
        main_bottom_html,
        calendars: cards,
        empty_state_html,
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::me::MeShell data=data/> }
    });
    handler(request).await
}

pub async fn logout(
    logout: OidcRpInitiatedLogout,
    State(state): State<AppState>,
    cfg: axum::Extension<AppConfig>,
) -> impl IntoResponse {
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

/// Public marketing landing page at `/` — ports the Stitch "Trang chủ" mockup
/// (project 7966553897766226544, screen eceda80d9000472cbd5e362a94e1bde1)
/// verbatim, with its placeholder nav/footer links (Tài liệu, Giá cả,
/// Security, Status — pages that don't exist) trimmed down to links that
/// actually go somewhere. `/` is otherwise the CalDAV protocol root (see
/// caldav.rs's `auth_middleware`/`handle_host_calendar`) — this only renders
/// for unauthenticated GET/HEAD requests on hosts with no personal calendar
/// alias, so calendar.opendiy.vn / mytime.opendiy.vn are unaffected.
pub async fn landing_page(
    lang: crate::i18n::Lang,
    request: axum::extract::Request,
) -> axum::response::Response {
    let data = match lang {
        crate::i18n::Lang::En => app::landing::en_data(),
        crate::i18n::Lang::Vi => app::landing::vi_data(),
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::landing::LandingShell data=data/> }
    });
    handler(request).await
}
