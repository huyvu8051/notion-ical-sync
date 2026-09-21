use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use tracing::error;

use crate::crypto::{decrypt_password, encrypt_password, generate_token, hash_password};
use crate::error_page::{error_page, OauthError};
use crate::session::{find_or_create_user, html_escape, owned_calendar_or_error, AppConfig};
use crate::AppState;

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

    let one_shot_plaintext_passwords = take_new_calendar_credentials(&session).await;
    let banner = if !one_shot_plaintext_passwords.is_empty() {
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

    let connect_errors = take_calendar_connect_errors(&session).await;
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
            let label = if name.is_empty() {
                public_id.as_str()
            } else {
                name.as_str()
            };
            let caldav_url = format!("{}/cal/{}", cfg.base_url, public_id);
            let password_row_html = match one_shot_plaintext_passwords.get(caldav_username) {
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

    let header_html = crate::i18n::top_nav_html(&email, lang, "/me");

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

pub async fn delete_calendar(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    if let Err(e) = sqlx::query("DELETE FROM calendars WHERE id = $1")
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to delete calendar {}: {}", cal.id, e);
        return error_page(lang, OauthError::FailedToDeleteCalendar);
    }

    let still_referenced: i64 =
        sqlx::query_scalar("SELECT count(*) FROM calendars WHERE database_id = $1")
            .bind(&cal.database_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(1);
    if still_referenced == 0 {
        state.cache.write().await.remove(&cal.database_id);
    }

    Redirect::to("/me").into_response()
}

pub async fn reveal_password(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let Some(key) = state.password_enc_key.as_ref() else {
        return error_page(lang, OauthError::RevealPasswordNotConfigured);
    };

    let encrypted: String =
        sqlx::query_scalar("SELECT caldav_password_encrypted FROM calendars WHERE id = $1")
            .bind(cal.id)
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();

    let Some(password) = (!encrypted.is_empty())
        .then(|| decrypt_password(key, &encrypted))
        .flatten()
    else {
        return error_page(lang, OauthError::PasswordPredatesReveal);
    };

    let stash = vec![(
        cal.display_name.clone(),
        cal.caldav_username.clone(),
        password,
    )];
    if let Err(e) = session.insert("new_calendar_credentials", &stash).await {
        error!("failed to stash revealed password in session: {}", e);
    }

    Redirect::to("/me").into_response()
}

pub async fn regenerate_password(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id, lang).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let new_password = generate_token(24);
    let Ok(password_hash) = hash_password(&new_password) else {
        return error_page(lang, OauthError::Generic);
    };
    let password_encrypted = state
        .password_enc_key
        .as_ref()
        .and_then(|key| encrypt_password(key, &new_password))
        .unwrap_or_default();

    if let Err(e) = sqlx::query("UPDATE calendars SET caldav_password_hash = $1, caldav_password_encrypted = $2 WHERE id = $3")
        .bind(&password_hash)
        .bind(&password_encrypted)
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to regenerate caldav password for calendar {}: {}", cal.id, e);
        return error_page(lang, OauthError::FailedToRegeneratePassword);
    }

    let stash = vec![(
        cal.display_name.clone(),
        cal.caldav_username.clone(),
        new_password,
    )];
    if let Err(e) = session.insert("new_calendar_credentials", &stash).await {
        error!("failed to stash regenerated password in session: {}", e);
    }

    Redirect::to("/me").into_response()
}

pub async fn take_new_calendar_credentials(
    session: &tower_sessions::Session,
) -> HashMap<String, String> {
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
}

pub async fn take_calendar_connect_errors(session: &tower_sessions::Session) -> Vec<String> {
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
}
