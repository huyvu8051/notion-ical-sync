use axum::{extract::State, http::{HeaderMap, StatusCode}, response::IntoResponse, Json};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::Deserialize;

use crate::i18n::Lang;
use crate::AppState;

#[derive(Clone)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl EmailConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            host: std::env::var("SMTP_HOST").ok()?,
            port: std::env::var("SMTP_PORT").ok()?.parse().ok()?,
            username: std::env::var("SMTP_USERNAME").ok()?,
            password: std::env::var("SMTP_PASSWORD").ok()?,
            from: std::env::var("EMAIL_FROM").ok()?,
        })
    }
}

fn stalwart_self_signed_tls_parameters(host: &str) -> Result<TlsParameters, String> {
    TlsParameters::builder(host.to_string())
        .dangerous_accept_invalid_certs(true)
        .dangerous_accept_invalid_hostnames(true)
        .build()
        .map_err(|e| format!("failed to configure TLS: {e}"))
}

pub async fn send_email(
    cfg: &EmailConfig,
    to: &str,
    subject: &str,
    html: &str,
) -> Result<(), String> {
    let email = Message::builder()
        .from(
            cfg.from
                .parse()
                .map_err(|e| format!("invalid from address: {e}"))?,
        )
        .to(to.parse().map_err(|e| format!("invalid to address: {e}"))?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html.to_string())
        .map_err(|e| format!("failed to build message: {e}"))?;

    let tls_parameters = stalwart_self_signed_tls_parameters(&cfg.host)?;

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
        .port(cfg.port)
        .tls(Tls::Wrapper(tls_parameters))
        .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
        .build();

    mailer
        .send(email)
        .await
        .map_err(|e| format!("failed to send email: {e}"))?;
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
<tr><td style="font-size:20px;font-weight:700;padding-bottom:24px;">NotionCal</td></tr>
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

    match send_email(
        &cfg,
        &body.to,
        "NotionCal test email",
        &wrap_in_email_template(
            "<p>This is a test email from NotionCal's admin test endpoint — if you're reading this, SMTP delivery is working.</p>",
        ),
    )
    .await
    {
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
