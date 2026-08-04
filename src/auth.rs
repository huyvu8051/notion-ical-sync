//! Keycloak/OIDC wiring for the SaaS's own login (separate from the Notion
//! OAuth in oauth.rs, which is *this app's* permission to read/write a
//! user's Notion workspace — two different identities, don't conflate them).
//! Pattern copied from biolink-vn/src/auth.rs, which has the same axum-oidc +
//! tower-sessions shape.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Redirect};
use axum_oidc::openidconnect::core::CoreGenderClaim;
use axum_oidc::openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims, OidcClient, OidcRpInitiatedLogout, OidcSession};

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

    async fn get(&self) -> Result<OidcSession<EmptyAdditionalClaims, CoreGenderClaim>, Self::Error> {
        Ok(self.0.get("axum-oidc").await?.unwrap_or_default())
    }

    async fn set(&mut self, value: OidcSession<EmptyAdditionalClaims, CoreGenderClaim>) -> Result<(), Self::Error> {
        self.0.insert("axum-oidc", value).await?;
        Ok(())
    }
}

pub async fn build_oidc_client(
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_url: String,
) -> OidcClient<EmptyAdditionalClaims> {
    let mut builder = OidcClient::<EmptyAdditionalClaims>::builder()
        .with_default_http_client()
        .with_redirect_url(
            redirect_url
                .parse()
                .unwrap_or_else(|_| panic!("invalid redirect url: {redirect_url}")),
        )
        .with_client_id(ClientId::new(client_id))
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()));

    if let Some(secret) = client_secret {
        builder = builder.with_client_secret(ClientSecret::new(secret));
    }

    builder
        .discover(IssuerUrl::new(issuer).expect("invalid KEYCLOAK_ISSUER_URL"))
        .await
        .expect("failed to discover Keycloak OIDC issuer — is it running?")
        .build()
}

/// Find-or-create the `users` row for this Keycloak login, returning its id.
pub async fn find_or_create_user(pool: &sqlx::PgPool, keycloak_sub: &str, email: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (keycloak_sub, email) VALUES ($1, $2)
         ON CONFLICT (keycloak_sub) DO UPDATE SET email = EXCLUDED.email
         RETURNING id",
    )
    .bind(keycloak_sub)
    .bind(email)
    .fetch_one(pool)
    .await
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

/// `/me` (dashboard) is styled to match the Stitch "Calendar của bạn" mockup
/// (Stitch project 7966553897766226544, screen 14ecf17308d644d68daa28eb5f3c50a0)
/// pixel-for-pixel — Tailwind CDN + the exact design-token config from that
/// screen, rather than the plain hand-rolled CSS the rest of the app uses.
pub(crate) const DASHBOARD_HEAD: &str = r##"
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&display=swap" rel="stylesheet">
<link href="https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet">
<script id="tailwind-config">
tailwind.config = {
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "error": "#ba1a1a", "surface-container-low": "#f5f3f3", "surface-container-high": "#e9e8e7",
        "surface-container-highest": "#e3e2e2", "on-surface-variant": "#444748"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  }
}
</script>
<style>
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.success-banner-gradient { background: linear-gradient(90deg, rgba(220, 252, 231, 0.5) 0%, rgba(220, 252, 231, 0.2) 100%); }
.error-banner-gradient { background: linear-gradient(90deg, rgba(254, 226, 226, 0.5) 0%, rgba(254, 226, 226, 0.2) 100%); }
</style>
"##;

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
    new_password_notice: "Mật khẩu CalDAV bên dưới sẽ không tự động hiển thị lại — lưu lại hoặc copy ngay.",
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
    delete_confirm: "Xoá calendar này? Dữ liệu trên Notion không bị ảnh hưởng, nhưng lịch sẽ ngừng đồng bộ.",
    empty_state: "Chưa có calendar nào — kết nối Notion để bắt đầu.",
    page_title: "Trang của bạn — NotionCal",
    logout_title: "Đăng xuất",
    heading: "Calendar của bạn",
    subheading: "Quản lý và đồng bộ hóa các cơ sở dữ liệu Notion với ứng dụng lịch yêu thích của bạn.",
    connect_more: "Kết nối thêm cơ sở dữ liệu",
    billing_free_until: "Miễn phí đến {date}",
    billing_subscribed: "Đã đăng ký — $1/năm",
    billing_quota: "{used}/{limit} sự kiện hôm nay",
    billing_upgrade_cta: "Nâng cấp $1/năm",
};

const ME_LABELS_EN: MeLabels = MeLabels {
    html_lang: "en",
    error_generic: "Something went wrong.",
    new_password_notice: "The CalDAV password below won't be shown again automatically — save or copy it now.",
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
    delete_confirm: "Delete this calendar? Your Notion data is untouched, but it will stop syncing.",
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
) -> impl IntoResponse {
    let l = match lang {
        crate::i18n::Lang::En => &ME_LABELS_EN,
        crate::i18n::Lang::Vi => &ME_LABELS_VI,
    };
    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();

    let user_id = match find_or_create_user(&state.db, sub, &email).await {
        Ok(id) => id,
        Err(_) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, l.error_generic).into_response(),
    };

    let billing_card = {
        let row: Option<(chrono::DateTime<chrono::Utc>, String)> =
            sqlx::query_as("SELECT trial_started_at, subscription_status FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let (trial_started_at, subscription_status) = row.unwrap_or((chrono::Utc::now(), "none".to_string()));
        let subscribed = matches!(subscription_status.as_str(), "trialing" | "active");

        let (status_text, show_cta) = if subscribed {
            (l.billing_subscribed.to_string(), false)
        } else {
            match crate::billing::effective_access(&state, user_id).await {
                crate::billing::AccessLevel::Unlimited => {
                    let free_until = crate::billing::trial_end(trial_started_at).format("%Y-%m-%d").to_string();
                    (l.billing_free_until.replace("{date}", &free_until), true)
                }
                crate::billing::AccessLevel::Quota { used_today, limit } => (
                    l.billing_quota.replace("{used}", &used_today.to_string()).replace("{limit}", &limit.to_string()),
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
        let names = connect_errors.iter().map(|n| html_escape(n)).collect::<Vec<_>>().join(", ");
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

    let items: String = calendars
        .iter()
        .map(|(public_id, name, caldav_username)| {
            let label = if name.is_empty() { public_id.as_str() } else { name.as_str() };
            let caldav_url = format!("{}/cal/{}", cfg.base_url, public_id);
            let password_row = match new_passwords.get(caldav_username) {
                Some(pw) => copy_row(l.caldav_password_label, pw),
                None => String::new(),
            };
            let public_id = html_escape(public_id);
            format!(
                r#"<div class="bg-surface border border-outline-variant rounded-lg p-lg hover:border-outline transition-colors duration-200">
<div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-md mb-lg">
<div class="flex items-center gap-sm">
<span class="text-h2">🗓️</span>
<h2 class="font-semibold text-h2">{label}</h2>
<span class="bg-[#DCFCE7] text-[#166534] px-xs py-[2px] rounded font-label-md text-[10px] uppercase tracking-wider">{active_badge}</span>
</div>
<a class="px-md h-8 border border-outline-variant hover:bg-surface-container-low font-label-md text-label-md transition-all flex items-center" href="/app/{public_id}">{open_calendar}</a>
</div>
{url_row}
{username_row}
{password_row}
<p class="text-on-surface-variant text-[13px] mt-sm">{paste_hint}</p>
<div class="flex items-center gap-md mt-md pt-md border-t border-outline-variant">
<form method="post" action="/me/calendars/{public_id}/reveal-password">
<button type="submit" class="text-label-md text-secondary hover:underline">{reveal_password}</button>
</form>
<form method="post" action="/me/calendars/{public_id}/regenerate-password" onsubmit="return confirm('{regenerate_confirm}');">
<button type="submit" class="text-label-md text-secondary hover:underline">{regenerate_password}</button>
</form>
<a href="/me/calendars/{public_id}/log" class="text-label-md text-secondary hover:underline">{view_log}</a>
<form method="post" action="/me/calendars/{public_id}/delete" onsubmit="return confirm('{delete_confirm}');" class="ml-auto">
<button type="submit" class="text-label-md text-error hover:underline">{delete}</button>
</form>
</div>
</div>"#,
                label = html_escape(label),
                url_row = copy_row(l.caldav_url_label, &caldav_url),
                username_row = copy_row(l.username_label, caldav_username),
                active_badge = l.active_badge,
                open_calendar = l.open_calendar,
                paste_hint = l.paste_hint,
                reveal_password = l.reveal_password,
                regenerate_confirm = l.regenerate_confirm,
                regenerate_password = l.regenerate_password,
                view_log = l.view_log,
                delete_confirm = l.delete_confirm,
                delete = l.delete,
            )
        })
        .collect();

    let content = if calendars.is_empty() {
        format!(
            r#"<div class="flex flex-col items-center justify-center py-xl text-center border border-dashed border-outline-variant rounded-lg">
<span class="material-symbols-outlined !text-[48px] text-outline mb-md">calendar_add_on</span>
<p class="text-body-lg text-on-surface-variant max-w-sm">{}</p>
</div>"#,
            l.empty_state
        )
    } else {
        format!(r#"<div class="space-y-md">{items}</div>"#)
    };

    let lang_toggle = crate::i18n::lang_toggle(lang, "/me");

    Html(format!(
        r#"<!doctype html>
<html lang="{html_lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{page_title}</title>{DASHBOARD_HEAD}</head>
<body class="bg-background text-on-surface font-body-md min-h-screen">
<header class="bg-surface border-b border-outline-variant sticky top-0 z-50">
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
</header>
<main class="max-w-[1280px] mx-auto px-margin-mobile md:px-margin-desktop py-lg space-y-lg">
{banner}
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
</div>
{content}
<p class="text-on-surface-variant text-[13px] pt-lg"><a class="underline hover:text-primary" href="/privacy">Privacy Policy</a> · <a class="underline hover:text-primary" href="/terms">Terms of Service</a></p>
</main>
<script>
function copyToClipboard(text, btn) {{
  navigator.clipboard.writeText(text).then(() => {{
    const icon = btn.querySelector('.material-symbols-outlined');
    const original = icon.innerText;
    icon.innerText = 'check';
    icon.classList.add('text-[#166534]');
    setTimeout(() => {{ icon.innerText = original; icon.classList.remove('text-[#166534]'); }}, 2000);
  }});
}}
</script>
</body></html>"#,
        html_lang = l.html_lang,
        page_title = l.page_title,
        lang_toggle = lang_toggle,
        logout_title = l.logout_title,
        heading = l.heading,
        subheading = l.subheading,
        connect_more = l.connect_more,
        email = html_escape(&email),
    ))
    .into_response()
}

pub async fn logout(logout: OidcRpInitiatedLogout, State(state): State<AppState>, cfg: axum::Extension<AppConfig>) -> impl IntoResponse {
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
pub async fn landing_page(lang: crate::i18n::Lang) -> impl IntoResponse {
    Html(match lang {
        crate::i18n::Lang::En => LANDING_PAGE_HTML_EN,
        crate::i18n::Lang::Vi => LANDING_PAGE_HTML_VI,
    })
}

const LANDING_PAGE_HTML_VI: &str = r##"<!doctype html>
<html class="light" lang="vi"><head>
<meta charset="utf-8"/>
<meta content="width=device-width, initial-scale=1.0" name="viewport"/>
<title>NotionCal - Đồng bộ hóa Notion với Calendar</title>
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet"/>
<script id="tailwind-config">
tailwind.config = {
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "secondary": "#0058be", "secondary-container": "#2170e4",
        "surface-container-low": "#f5f3f3", "surface-container": "#efeded"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px", "gutter": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "display": ["32px", { lineHeight: "1.2", letterSpacing: "-0.02em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h3": ["16px", { lineHeight: "1.5", letterSpacing: "-0.01em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }]
      }
    }
  }
}
</script>
<style>
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
body { font-family: 'Inter', sans-serif; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
.bento-grid { display: grid; grid-template-columns: repeat(12, 1fr); gap: 1rem; }
.hairline-border { border: 1px solid #E5E5E5; }
.hover-lift:hover { transform: translateY(-2px); transition: transform 0.2s ease-out; border-color: #D4D4D4; }
.glass-header { backdrop-filter: blur(8px); background: rgba(251, 249, 249, 0.85); }
</style>
</head>
<body class="bg-background text-on-surface">
<header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
<div class="flex items-center gap-8">
<a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
<span class="material-symbols-outlined text-primary">calendar_month</span>
NotionCal
</a>
<nav class="hidden md:flex items-center gap-6">
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#how-it-works">Cách hoạt động</a>
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#pricing">Giá cả</a>
</nav>
</div>
<div class="flex items-center gap-4">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-2 py-2" href="/lang/en?next=/">EN</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-4 py-2" href="/me">Đăng nhập</a>
<a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me">Đăng ký</a>
</div>
</div>
</header>
<main class="pt-[64px]">
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl md:py-[120px]">
<div class="grid grid-cols-1 lg:grid-cols-2 gap-xl items-center">
<div class="space-y-lg">
<div class="inline-flex items-center gap-2 px-3 py-1 bg-surface-container rounded border border-outline-variant text-on-surface-variant">
<span class="material-symbols-outlined text-[16px]">sync</span>
<span class="text-[12px] uppercase tracking-wider font-bold">Đồng bộ hai chiều</span>
</div>
<h1 class="text-[48px] md:text-[64px] leading-tight text-primary font-extrabold tracking-tight">
Biến cơ sở dữ liệu Notion thành lịch trực tuyến
</h1>
<p class="text-body-lg text-on-surface-variant max-w-[540px]">
Đồng bộ hóa Notion của bạn với Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào. Chỉnh sửa linh hoạt từ cả hai phía.
</p>
<div class="pt-sm">
<a class="bg-primary text-on-primary h-[48px] px-8 rounded-lg text-h3 inline-flex items-center gap-3 hover:opacity-90 transition-all active:scale-[0.98]" href="/me">
<span class="material-symbols-outlined" style="font-variation-settings: 'FILL' 1;">login</span>
Đăng nhập / Đăng ký
</a>
<p class="mt-4 text-label-md text-on-surface-variant flex items-center gap-2">
<span class="material-symbols-outlined text-[16px] text-secondary">verified</span>
6 tháng đầu miễn phí, không giới hạn. Không cần thẻ tín dụng.
</p>
</div>
</div>
<div class="relative group">
<div class="absolute -inset-4 bg-gradient-to-r from-secondary-container/10 to-primary/5 rounded-xl blur-2xl group-hover:opacity-75 transition-opacity"></div>
<div class="relative rounded-xl hairline-border overflow-hidden bg-white shadow-sm p-lg">
<div class="flex items-center justify-center h-64 bg-surface-container rounded-lg border border-dashed border-outline-variant">
<span class="material-symbols-outlined !text-[64px] text-outline opacity-40">calendar_month</span>
</div>
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="how-it-works">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Quy trình đơn giản</h2>
<p class="text-body-md text-on-surface-variant mt-2">Bắt đầu đồng bộ hóa dữ liệu của bạn chỉ với 3 bước</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-3 gap-gutter">
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">link</span>
</div>
<h3 class="text-h3 text-primary mb-2">1. Kết nối Notion</h3>
<p class="text-body-md text-on-surface-variant">Kết nối không gian làm việc Notion của bạn một cách bảo mật thông qua OAuth.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">database</span>
</div>
<h3 class="text-h3 text-primary mb-2">2. Chọn cơ sở dữ liệu</h3>
<p class="text-body-md text-on-surface-variant">Chọn cơ sở dữ liệu bạn muốn đồng bộ. Yêu cầu có thuộc tính kiểu Date.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">event_available</span>
</div>
<h3 class="text-h3 text-primary mb-2">3. Đồng bộ lịch</h3>
<p class="text-body-md text-on-surface-variant">Đăng ký trên ứng dụng lịch của bạn (Apple, Google, Outlook) thông qua link CalDAV.</p>
</div>
</div>
</div>
</section>
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl">
<div class="bento-grid grid-rows-2">
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-white flex flex-col justify-between">
<div>
<h4 class="text-h2 text-primary mb-2">Đồng bộ hai chiều thời gian thực</h4>
<p class="text-body-md text-on-surface-variant">Thay đổi trên Notion sẽ cập nhật ngay lập tức trên Lịch của bạn và ngược lại. Không bao giờ bỏ lỡ một deadline nào.</p>
</div>
<div class="mt-xl h-32 bg-surface-container rounded-lg border border-dashed border-outline flex items-center justify-center">
<span class="material-symbols-outlined text-[48px] text-outline opacity-40">sync_alt</span>
</div>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-surface-container flex flex-col justify-center items-center text-center">
<span class="material-symbols-outlined text-[48px] mb-4 text-secondary">security</span>
<h4 class="text-h3 text-primary">Dữ liệu của bạn, do bạn kiểm soát</h4>
<p class="text-body-md text-on-surface-variant mt-2">Chỉ đọc/ghi vào các trang bạn cho phép. Có thể ngắt kết nối bất cứ lúc nào.</p>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-white">
<h4 class="text-h3 text-primary mb-2">Giá đơn giản</h4>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> 6 tháng đầu miễn phí, không giới hạn
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Sau đó chỉ $1/năm
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không cần thẻ tín dụng để bắt đầu
</li>
</ul>
</div>
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-primary text-on-primary flex items-center justify-between">
<div>
<h4 class="text-h2 mb-1">Dành cho mọi ứng dụng lịch</h4>
<p class="text-label-md opacity-80 uppercase tracking-widest">Chuẩn giao thức CalDAV</p>
</div>
<div class="text-code bg-white/10 p-3 rounded hairline-border border-white/20">
notion-caldav.opendiy.vn/cal/...
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="pricing">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Giá cả</h2>
<p class="text-body-md text-on-surface-variant mt-2">Dùng thử miễn phí 6 tháng, sau đó chỉ $1/năm</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-2 gap-gutter max-w-[720px] mx-auto">
<div class="bg-white p-lg hairline-border rounded-xl">
<h3 class="text-h2 text-primary mb-1">6 tháng đầu</h3>
<p class="text-display text-primary mb-2">Miễn phí</p>
<p class="text-body-md text-on-surface-variant mb-md">Dành cho mọi tài khoản mới, tính từ ngày đăng ký.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không giới hạn số sự kiện
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Kết nối nhiều database
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không cần thẻ tín dụng
</li>
</ul>
</div>
<div class="bg-primary text-on-primary p-lg hairline-border rounded-xl relative">
<div class="absolute -top-3 right-lg bg-secondary text-on-primary text-[11px] uppercase tracking-wider font-bold px-3 py-1 rounded-full">Sau 6 tháng</div>
<h3 class="text-h2 mb-1">Nâng cấp</h3>
<p class="text-display mb-2">$1<span class="text-h3 opacity-70">/năm</span></p>
<p class="text-body-md opacity-80 mb-md">Đăng ký sớm vẫn được hưởng trọn 6 tháng miễn phí — chỉ bắt đầu tính phí sau khi hết hạn.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Không giới hạn số sự kiện
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Thanh toán an toàn qua Stripe
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Huỷ bất cứ lúc nào
</li>
</ul>
</div>
</div>
<p class="text-center text-label-md text-on-surface-variant mt-lg">Hết 6 tháng miễn phí mà chưa đăng ký? Bạn vẫn dùng được, giới hạn 10 sự kiện mới/ngày.</p>
</div>
</section>
</main>
<footer class="border-t border-outline-variant bg-surface mt-xl">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop py-lg flex flex-col md:flex-row justify-between items-center gap-lg">
<div class="flex flex-col md:flex-row items-center gap-md">
<span class="text-h3 font-bold text-primary">NotionCal</span>
</div>
<div class="flex items-center gap-6">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/privacy">Chính sách bảo mật</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/terms">Điều khoản dịch vụ</a>
</div>
</div>
</footer>
</body></html>"##;

const LANDING_PAGE_HTML_EN: &str = r##"<!doctype html>
<html class="light" lang="en"><head>
<meta charset="utf-8"/>
<meta content="width=device-width, initial-scale=1.0" name="viewport"/>
<title>NotionCal - Sync Notion with your Calendar</title>
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist:wght@400;500&family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet"/>
<script id="tailwind-config">
tailwind.config = {
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "secondary": "#0058be", "secondary-container": "#2170e4",
        "surface-container-low": "#f5f3f3", "surface-container": "#efeded"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px", "gutter": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "display": ["32px", { lineHeight: "1.2", letterSpacing: "-0.02em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h3": ["16px", { lineHeight: "1.5", letterSpacing: "-0.01em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }]
      }
    }
  }
}
</script>
<style>
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
body { font-family: 'Inter', sans-serif; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
.bento-grid { display: grid; grid-template-columns: repeat(12, 1fr); gap: 1rem; }
.hairline-border { border: 1px solid #E5E5E5; }
.hover-lift:hover { transform: translateY(-2px); transition: transform 0.2s ease-out; border-color: #D4D4D4; }
.glass-header { backdrop-filter: blur(8px); background: rgba(251, 249, 249, 0.85); }
</style>
</head>
<body class="bg-background text-on-surface">
<header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
<div class="flex items-center gap-8">
<a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
<span class="material-symbols-outlined text-primary">calendar_month</span>
NotionCal
</a>
<nav class="hidden md:flex items-center gap-6">
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#how-it-works">How it works</a>
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#pricing">Pricing</a>
</nav>
</div>
<div class="flex items-center gap-4">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-2 py-2" href="/lang/vi?next=/">VI</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-4 py-2" href="/me">Log in</a>
<a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me">Sign up</a>
</div>
</div>
</header>
<main class="pt-[64px]">
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl md:py-[120px]">
<div class="grid grid-cols-1 lg:grid-cols-2 gap-xl items-center">
<div class="space-y-lg">
<div class="inline-flex items-center gap-2 px-3 py-1 bg-surface-container rounded border border-outline-variant text-on-surface-variant">
<span class="material-symbols-outlined text-[16px]">sync</span>
<span class="text-[12px] uppercase tracking-wider font-bold">Two-way sync</span>
</div>
<h1 class="text-[48px] md:text-[64px] leading-tight text-primary font-extrabold tracking-tight">
Turn your Notion database into an online calendar
</h1>
<p class="text-body-lg text-on-surface-variant max-w-[540px]">
Sync your Notion workspace with Apple Calendar, Google Calendar, or any CalDAV app. Edit freely from either side.
</p>
<div class="pt-sm">
<a class="bg-primary text-on-primary h-[48px] px-8 rounded-lg text-h3 inline-flex items-center gap-3 hover:opacity-90 transition-all active:scale-[0.98]" href="/me">
<span class="material-symbols-outlined" style="font-variation-settings: 'FILL' 1;">login</span>
Log in / Sign up
</a>
<p class="mt-4 text-label-md text-on-surface-variant flex items-center gap-2">
<span class="material-symbols-outlined text-[16px] text-secondary">verified</span>
Free for your first 6 months, unlimited. No credit card required.
</p>
</div>
</div>
<div class="relative group">
<div class="absolute -inset-4 bg-gradient-to-r from-secondary-container/10 to-primary/5 rounded-xl blur-2xl group-hover:opacity-75 transition-opacity"></div>
<div class="relative rounded-xl hairline-border overflow-hidden bg-white shadow-sm p-lg">
<div class="flex items-center justify-center h-64 bg-surface-container rounded-lg border border-dashed border-outline-variant">
<span class="material-symbols-outlined !text-[64px] text-outline opacity-40">calendar_month</span>
</div>
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="how-it-works">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">A simple process</h2>
<p class="text-body-md text-on-surface-variant mt-2">Start syncing your data in just 3 steps</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-3 gap-gutter">
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">link</span>
</div>
<h3 class="text-h3 text-primary mb-2">1. Connect Notion</h3>
<p class="text-body-md text-on-surface-variant">Securely connect your Notion workspace via OAuth.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">database</span>
</div>
<h3 class="text-h3 text-primary mb-2">2. Pick a database</h3>
<p class="text-body-md text-on-surface-variant">Choose the database you want to sync. Requires a Date property.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">event_available</span>
</div>
<h3 class="text-h3 text-primary mb-2">3. Sync your calendar</h3>
<p class="text-body-md text-on-surface-variant">Subscribe from your calendar app (Apple, Google, Outlook) via the CalDAV link.</p>
</div>
</div>
</div>
</section>
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl">
<div class="bento-grid grid-rows-2">
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-white flex flex-col justify-between">
<div>
<h4 class="text-h2 text-primary mb-2">Real-time two-way sync</h4>
<p class="text-body-md text-on-surface-variant">Changes in Notion update your calendar instantly, and vice versa. Never miss a deadline.</p>
</div>
<div class="mt-xl h-32 bg-surface-container rounded-lg border border-dashed border-outline flex items-center justify-center">
<span class="material-symbols-outlined text-[48px] text-outline opacity-40">sync_alt</span>
</div>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-surface-container flex flex-col justify-center items-center text-center">
<span class="material-symbols-outlined text-[48px] mb-4 text-secondary">security</span>
<h4 class="text-h3 text-primary">Your data, your control</h4>
<p class="text-body-md text-on-surface-variant mt-2">Only reads/writes the pages you authorize. Disconnect anytime.</p>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-white">
<h4 class="text-h3 text-primary mb-2">Simple pricing</h4>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Free for your first 6 months, unlimited
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Just $1/year after that
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> No credit card required to start
</li>
</ul>
</div>
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-primary text-on-primary flex items-center justify-between">
<div>
<h4 class="text-h2 mb-1">Works with any calendar app</h4>
<p class="text-label-md opacity-80 uppercase tracking-widest">Standard CalDAV protocol</p>
</div>
<div class="text-code bg-white/10 p-3 rounded hairline-border border-white/20">
notion-caldav.opendiy.vn/cal/...
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="pricing">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Pricing</h2>
<p class="text-body-md text-on-surface-variant mt-2">Free for 6 months, then just $1/year</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-2 gap-gutter max-w-[720px] mx-auto">
<div class="bg-white p-lg hairline-border rounded-xl">
<h3 class="text-h2 text-primary mb-1">First 6 months</h3>
<p class="text-display text-primary mb-2">Free</p>
<p class="text-body-md text-on-surface-variant mb-md">For every new account, starting the day you sign up.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Unlimited events
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Connect multiple databases
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> No credit card required
</li>
</ul>
</div>
<div class="bg-primary text-on-primary p-lg hairline-border rounded-xl relative">
<div class="absolute -top-3 right-lg bg-secondary text-on-primary text-[11px] uppercase tracking-wider font-bold px-3 py-1 rounded-full">After 6 months</div>
<h3 class="text-h2 mb-1">Upgrade</h3>
<p class="text-display mb-2">$1<span class="text-h3 opacity-70">/year</span></p>
<p class="text-body-md opacity-80 mb-md">Subscribe early and you'll still get your full 6 free months first — billing only starts once your trial ends.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Unlimited events
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Secure checkout via Stripe
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Cancel anytime
</li>
</ul>
</div>
</div>
<p class="text-center text-label-md text-on-surface-variant mt-lg">Past your free 6 months without subscribing? You can still use NotionCal, capped at 10 new events/day.</p>
</div>
</section>
</main>
<footer class="border-t border-outline-variant bg-surface mt-xl">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop py-lg flex flex-col md:flex-row justify-between items-center gap-lg">
<div class="flex flex-col md:flex-row items-center gap-md">
<span class="text-h3 font-bold text-primary">NotionCal</span>
</div>
<div class="flex items-center gap-6">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/privacy">Privacy Policy</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/terms">Terms of Service</a>
</div>
</div>
</footer>
</body></html>"##;
