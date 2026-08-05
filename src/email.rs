//! Transactional email — self-hosted via Stalwart (see infra plan), not a
//! third-party SaaS. Templates are plain inline-styled HTML: unlike the
//! webview/landing pages, email clients don't run `<script>` or fetch
//! external stylesheets, so the app's usual Tailwind-CDN approach doesn't
//! apply here.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::i18n::Lang;

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

pub async fn send_email(cfg: &EmailConfig, to: &str, subject: &str, html: &str) -> Result<(), String> {
    let email = Message::builder()
        .from(cfg.from.parse().map_err(|e| format!("invalid from address: {e}"))?)
        .to(to.parse().map_err(|e| format!("invalid to address: {e}"))?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html.to_string())
        .map_err(|e| format!("failed to build message: {e}"))?;

    // Stalwart has no ACME cert (no inbound path for HTTP-01), so it presents a
    // self-signed one — `dangerous_accept_invalid_certs` skips validation for it.
    // Safe here: this is an internal, same-cluster, trusted-network hop, not the
    // public internet. STARTTLS is still required (`builder_dangerous` alone would
    // negotiate no encryption at all, which Stalwart would likely refuse AUTH over).
    let tls_parameters = TlsParameters::builder(cfg.host.clone())
        .dangerous_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("failed to configure TLS: {e}"))?;

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
        .port(cfg.port)
        .tls(Tls::Required(tls_parameters))
        .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
        .build();

    mailer.send(email).await.map_err(|e| format!("failed to send email: {e}"))?;
    Ok(())
}

/// Fire-and-forget send used from spots that must never fail the actual
/// request (welcome email on signup, billing notices from the Stripe
/// webhook) — same posture as `AppState::log_sync`.
pub fn spawn_send(cfg: EmailConfig, to: String, subject: String, html: String) {
    tokio::spawn(async move {
        if let Err(e) = send_email(&cfg, &to, &subject, &html).await {
            tracing::error!("failed to send email to {}: {}", to, e);
        }
    });
}

fn wrapper(body: &str) -> String {
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

pub fn welcome_email(lang: Lang) -> (&'static str, String) {
    match lang {
        Lang::Vi => (
            "Chào mừng đến với NotionCal",
            wrapper(
                r#"<p>Chào bạn,</p>
<p>Cảm ơn bạn đã đăng ký NotionCal. Bạn đang có <strong>6 tháng miễn phí, không giới hạn</strong> để đồng bộ Notion với lịch của mình.</p>
<p>Bắt đầu ngay: kết nối Notion và chọn database bạn muốn đồng bộ tại <a href="https://notion-caldav.opendiy.vn/me">trang của bạn</a>.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Welcome to NotionCal",
            wrapper(
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
            wrapper(&format!(
                r#"<p>Chào bạn,</p>
<p>6 tháng miễn phí của bạn sẽ hết hạn vào <strong>{free_until}</strong>. Sau đó, nếu chưa đăng ký, tài khoản của bạn sẽ bị giới hạn 10 sự kiện mới/ngày.</p>
<p>Nâng cấp $1/năm để tiếp tục không giới hạn: <a href="https://notion-caldav.opendiy.vn/billing/checkout">nâng cấp ngay</a>.</p>
<p>— Đội ngũ NotionCal</p>"#
            )),
        ),
        Lang::En => (
            "Your free 6 months are ending soon",
            wrapper(&format!(
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
            wrapper(
                r#"<p>Chào bạn,</p>
<p>Bạn đã đăng ký gói $1/năm thành công. Nếu bạn đang trong 6 tháng miễn phí, chưa bị tính phí ngay — việc thanh toán chỉ bắt đầu sau khi hết 6 tháng.</p>
<p>Cảm ơn bạn đã đồng hành cùng NotionCal.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "You're subscribed",
            wrapper(
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
            wrapper(
                r#"<p>Chào bạn,</p>
<p>Chúng tôi không thể thu phí $1/năm cho tài khoản của bạn. Vui lòng kiểm tra và cập nhật phương thức thanh toán trên Stripe để tránh gián đoạn dịch vụ.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Your payment failed",
            wrapper(
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
            wrapper(
                r#"<p>Chào bạn,</p>
<p>Gói $1/năm của bạn đã bị huỷ. Nếu đã hết 6 tháng miễn phí, tài khoản của bạn sẽ bị giới hạn 10 sự kiện mới/ngày cho đến khi đăng ký lại.</p>
<p>Đăng ký lại bất cứ lúc nào tại <a href="https://notion-caldav.opendiy.vn/billing/checkout">đây</a>.</p>
<p>— Đội ngũ NotionCal</p>"#,
            ),
        ),
        Lang::En => (
            "Your subscription was canceled",
            wrapper(
                r#"<p>Hi there,</p>
<p>Your $1/year subscription was canceled. If your free 6 months have already ended, your account is now capped at 10 new events/day until you resubscribe.</p>
<p>Resubscribe anytime <a href="https://notion-caldav.opendiy.vn/billing/checkout">here</a>.</p>
<p>— The NotionCal team</p>"#,
            ),
        ),
    }
}
