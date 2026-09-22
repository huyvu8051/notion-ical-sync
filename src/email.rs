use axum::{extract::State, http::{HeaderMap, StatusCode}, response::IntoResponse, Json};
use serde::Deserialize;

use crate::i18n::Lang;
use crate::AppState;

const RESEND_API_URL: &str = "https://api.resend.com/emails";

#[derive(Clone)]
pub struct EmailConfig {
    pub api_key: String,
    pub from: String,
}

impl EmailConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            api_key: std::env::var("RESEND_API_KEY").ok()?,
            from: std::env::var("EMAIL_FROM").ok()?,
        })
    }
}

pub async fn send_email(
    cfg: &EmailConfig,
    to: &str,
    subject: &str,
    html: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(RESEND_API_URL)
        .bearer_auth(&cfg.api_key)
        .json(&serde_json::json!({
            "from": cfg.from,
            "to": [to],
            "subject": subject,
            "html": html,
        }))
        .send()
        .await
        .map_err(|e| format!("failed to reach Resend: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Resend rejected the email ({status}): {body}"));
    }
    Ok(())
}

pub fn spawn_send(cfg: EmailConfig, to: String, subject: String, html: String) {
    tokio::spawn(async move {
        if let Err(e) = send_email(&cfg, &to, &subject, &html).await {
            tracing::error!("failed to send email to {}: {}", to, e);
        }
    });
}

fn wrap_in_email_template(body: &str) -> String {
    format!(
        r#"<!doctype html>
<html><body style="margin:0;padding:0;background:#fbf9f9;font-family:Arial,Helvetica,sans-serif;color:#1b1c1c;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:32px 16px;">
<tr><td align="center">
<table role="presentation" width="480" cellpadding="0" cellspacing="0" style="max-width:480px;background:#ffffff;border:1px solid #e5e5e5;border-radius:8px;padding:32px;">
<tr><td style="padding-bottom:24px;">
<table role="presentation" cellpadding="0" cellspacing="0"><tr>
<td style="padding-right:8px;"><img src="https://notion-caldav.opendiy.vn/static/logo-email.png" width="28" height="28" alt="" style="display:block;"/></td>
<td style="font-size:20px;font-weight:700;vertical-align:middle;">NotionCal</td>
</tr></table>
</td></tr>
<tr><td style="font-size:14px;line-height:1.6;">{body}</td></tr>
</table>
</td></tr>
</table>
</body></html>"#
    )
}

#[derive(Deserialize)]
pub struct SendTestEmailRequest {
    to: String,
    template: Option<String>,
}

pub async fn send_test_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SendTestEmailRequest>,
) -> impl IntoResponse {
    let Some(expected_secret) = state.admin_secret.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let provided = headers.get("X-Admin-Secret").and_then(|v| v.to_str().ok());
    if provided != Some(expected_secret) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(cfg) = state.email.clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "SMTP not configured").into_response();
    };

    let (subject, html) = match body.template.as_deref() {
        Some("lifetime_promo") => lifetime_promo_email(),
        _ => (
            "NotionCal test email",
            wrap_in_email_template(
                "<p>This is a test email from NotionCal's admin test endpoint — if you're reading this, SMTP delivery is working.</p>",
            ),
        ),
    };

    match send_email(&cfg, &body.to, subject, &html).await {
        Ok(()) => Json(serde_json::json!({ "sent_to": body.to })).into_response(),
        Err(e) => {
            tracing::error!("send_test_email: failed to send to {}: {}", body.to, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to send: {e}"),
            )
                .into_response()
        }
    }
}

pub fn lifetime_promo_email() -> (&'static str, String) {
    (
        "You're free to use NotionCal through 2027 / Miễn phí đến 12/2027",
        wrap_in_email_template(
            r#"<p>Hi there,</p>
<p>Thanks for being with NotionCal since the early days. As a thank-you, we've given your account <strong>unlimited, free access through the end of December 2027</strong> — no card required, no daily event cap.</p>
<p>Nothing to do on your end — this is already active on your account.</p>
<p>Check your calendars <a href="https://notion-caldav.opendiy.vn/me">here</a>.</p>
<p>— The NotionCal team</p>
<hr style="border:none;border-top:1px solid #e5e5e5;margin:24px 0;"/>
<p>Chào bạn,</p>
<p>Cảm ơn bạn đã đồng hành cùng NotionCal từ những ngày đầu. Để tri ân, chúng tôi quyết định cho bạn <strong>sử dụng không giới hạn, miễn phí đến hết tháng 12/2027</strong> — không cần nhập thẻ, không giới hạn số sự kiện đồng bộ mỗi ngày.</p>
<p>Không cần làm gì thêm — tài khoản của bạn đã được kích hoạt ưu đãi này.</p>
<p>Xem lại calendar của bạn tại <a href="https://notion-caldav.opendiy.vn/me">đây</a>.</p>
<p>— Đội ngũ NotionCal</p>"#,
        ),
    )
}

pub fn welcome_email(lang: Lang) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "Chào mừng đến với NotionCal",
            wrap_in_email_template(
                r#"<p>Chào bạn,</p>
<p>Cảm ơn bạn đã đăng ký NotionCal. Bạn đang có <strong>6 tháng miễn phí, không giới hạn</strong> để đồng bộ Notion với lịch của mình.</p>
<p>Bắt đầu ngay: kết nối Notion và chọn database bạn muốn đồng bộ tại <a href="https://notion-caldav.opendiy.vn/me">trang của bạn</a>.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Welcome to NotionCal",
            wrap_in_email_template(
                r#"<p>Hi there,</p>
<p>Thanks for signing up for NotionCal. You've got <strong>6 free, unlimited months</strong> to sync Notion with your calendar.</p>
<p>Get started: connect Notion and pick a database to sync from <a href="https://notion-caldav.opendiy.vn/me">your dashboard</a>.</p>
<p>— The NotionCal team</p>"#,
            ),
        ),
    }
}

pub fn trial_ending_email(lang: Lang, free_until: &str) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "6 tháng miễn phí của bạn sắp hết hạn",
            wrap_in_email_template(&format!(
                r#"<p>Chào bạn,</p>
<p>6 tháng miễn phí của bạn sẽ hết hạn vào <strong>{free_until}</strong>. Sau đó, nếu chưa đăng ký, tài khoản của bạn sẽ bị giới hạn 10 sự kiện mới/ngày.</p>
<p>Nâng cấp $1/năm để tiếp tục không giới hạn: <a href="https://notion-caldav.opendiy.vn/billing/checkout">nâng cấp ngay</a>.</p>
<p>— Đội ngũ NotionCal</p>"#
            )),
        ),
        Lang::En => (
            "Your free 6 months are ending soon",
            wrap_in_email_template(&format!(
                r#"<p>Hi there,</p>
<p>Your free 6 months end on <strong>{free_until}</strong>. After that, if you haven't subscribed, your account will be capped at 10 new events/day.</p>
<p>Upgrade for $1/year to stay unlimited: <a href="https://notion-caldav.opendiy.vn/billing/checkout">upgrade now</a>.</p>
<p>— The NotionCal team</p>"#
            )),
        ),
    }
}

pub fn subscribed_email(lang: Lang) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "Đăng ký thành công",
            wrap_in_email_template(
                r#"<p>Chào bạn,</p>
<p>Bạn đã đăng ký gói $1/năm thành công. Nếu bạn đang trong 6 tháng miễn phí, chưa bị tính phí ngay — việc thanh toán chỉ bắt đầu sau khi hết 6 tháng.</p>
<p>Cảm ơn bạn đã đồng hành cùng NotionCal.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "You're subscribed",
            wrap_in_email_template(
                r#"<p>Hi there,</p>
<p>Your $1/year subscription is confirmed. If you're still inside your free 6 months, you won't be charged yet — billing only starts once that period ends.</p>
<p>Thanks for using NotionCal.</p>
<p>— The NotionCal team</p>"#,
            ),
        ),
    }
}

pub fn payment_failed_email(lang: Lang) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "Thanh toán không thành công",
            wrap_in_email_template(
                r#"<p>Chào bạn,</p>
<p>Chúng tôi không thể thu phí $1/năm cho tài khoản của bạn. Vui lòng kiểm tra và cập nhật phương thức thanh toán trên Stripe để tránh gián đoạn dịch vụ.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Your payment failed",
            wrap_in_email_template(
                r#"<p>Hi there,</p>
<p>We couldn't charge your $1/year subscription. Please check and update your payment method on Stripe to avoid any interruption.</p>
<p>— The NotionCal team</p>"#,
            ),
        ),
    }
}

pub fn subscription_canceled_email(lang: Lang) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "Gói đăng ký của bạn đã bị huỷ",
            wrap_in_email_template(
                r#"<p>Chào bạn,</p>
<p>Gói $1/năm của bạn đã bị huỷ. Nếu đã hết 6 tháng miễn phí, tài khoản của bạn sẽ bị giới hạn 10 sự kiện mới/ngày cho đến khi đăng ký lại.</p>
<p>Đăng ký lại bất cứ lúc nào tại <a href="https://notion-caldav.opendiy.vn/billing/checkout">đây</a>.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Your subscription was canceled",
            wrap_in_email_template(
                r#"<p>Hi there,</p>
<p>Your $1/year subscription was canceled. If your free 6 months have already ended, your account is now capped at 10 new events/day until you resubscribe.</p>
<p>Resubscribe anytime <a href="https://notion-caldav.opendiy.vn/billing/checkout">here</a>.</p>
<p>— The NotionCal team</p>"#,
            ),
        ),
    }
}
