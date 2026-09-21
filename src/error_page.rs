use axum::response::{Html, IntoResponse};

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

pub(crate) enum OauthError {
    NotionNotConfigured,
    TryAgain,
    NotionDenied(String),
    MissingAuthCode,
    InvalidSession,
    CantReachNotion,
    NotionRejectedToken,
    InvalidNotionResponse,
    NotionResponseMissingFields,
    Generic,
    FailedToSaveConnection,
    ConnectionNotFound,
    FailedToListDatabases,
    InvalidRequest,
    CalendarNotFound,
    AccessDenied,
    FailedToDeleteCalendar,
    FailedToRegeneratePassword,
    BillingNotConfigured,
    FailedToCreateCheckoutSession,
}

impl OauthError {
    fn message(&self, lang: crate::i18n::Lang) -> String {
        use crate::i18n::Lang;
        match (self, lang) {
            (Self::NotionNotConfigured, Lang::Vi) => "Notion OAuth chưa được cấu hình trên server này.".to_string(),
            (Self::NotionNotConfigured, Lang::En) => "Notion OAuth isn't configured on this server.".to_string(),
            (Self::TryAgain, Lang::Vi) => "Có lỗi xảy ra, vui lòng thử lại.".to_string(),
            (Self::TryAgain, Lang::En) => "Something went wrong, please try again.".to_string(),
            (Self::NotionDenied(reason), Lang::Vi) => format!("Notion từ chối cấp quyền: {reason}"),
            (Self::NotionDenied(reason), Lang::En) => format!("Notion denied the authorization request: {reason}"),
            (Self::MissingAuthCode, Lang::Vi) => "Thiếu mã xác thực từ Notion.".to_string(),
            (Self::MissingAuthCode, Lang::En) => "Missing authorization code from Notion.".to_string(),
            (Self::InvalidSession, Lang::Vi) => "Phiên xác thực không hợp lệ, vui lòng thử lại.".to_string(),
            (Self::InvalidSession, Lang::En) => "Invalid auth session, please try again.".to_string(),
            (Self::CantReachNotion, Lang::Vi) => "Không thể kết nối tới Notion.".to_string(),
            (Self::CantReachNotion, Lang::En) => "Couldn't connect to Notion.".to_string(),
            (Self::NotionRejectedToken, Lang::Vi) => "Notion từ chối yêu cầu trao đổi token.".to_string(),
            (Self::NotionRejectedToken, Lang::En) => "Notion rejected the token exchange request.".to_string(),
            (Self::InvalidNotionResponse, Lang::Vi) => "Phản hồi từ Notion không hợp lệ.".to_string(),
            (Self::InvalidNotionResponse, Lang::En) => "Invalid response from Notion.".to_string(),
            (Self::NotionResponseMissingFields, Lang::Vi) => "Phản hồi từ Notion thiếu access_token hoặc workspace_id.".to_string(),
            (Self::NotionResponseMissingFields, Lang::En) => "Notion's response is missing access_token or workspace_id.".to_string(),
            (Self::Generic, Lang::Vi) => "Có lỗi xảy ra.".to_string(),
            (Self::Generic, Lang::En) => "Something went wrong.".to_string(),
            (Self::FailedToSaveConnection, Lang::Vi) => "Không thể lưu kết nối Notion.".to_string(),
            (Self::FailedToSaveConnection, Lang::En) => "Failed to save the Notion connection.".to_string(),
            (Self::ConnectionNotFound, Lang::Vi) => "Không tìm thấy kết nối Notion này.".to_string(),
            (Self::ConnectionNotFound, Lang::En) => "This Notion connection wasn't found.".to_string(),
            (Self::FailedToListDatabases, Lang::Vi) => "Không thể lấy danh sách cơ sở dữ liệu từ Notion.".to_string(),
            (Self::FailedToListDatabases, Lang::En) => "Failed to list databases from Notion.".to_string(),
            (Self::InvalidRequest, Lang::Vi) => "Yêu cầu không hợp lệ.".to_string(),
            (Self::InvalidRequest, Lang::En) => "Invalid request.".to_string(),
            (Self::CalendarNotFound, Lang::Vi) => "Không tìm thấy calendar này.".to_string(),
            (Self::CalendarNotFound, Lang::En) => "This calendar wasn't found.".to_string(),
            (Self::AccessDenied, Lang::Vi) => "Bạn không có quyền truy cập calendar này.".to_string(),
            (Self::AccessDenied, Lang::En) => "You don't have access to this calendar.".to_string(),
            (Self::FailedToDeleteCalendar, Lang::Vi) => "Không thể xoá calendar này.".to_string(),
            (Self::FailedToDeleteCalendar, Lang::En) => "Failed to delete this calendar.".to_string(),
            (Self::FailedToRegeneratePassword, Lang::Vi) => "Không thể tạo lại mật khẩu.".to_string(),
            (Self::FailedToRegeneratePassword, Lang::En) => "Failed to regenerate the password.".to_string(),
            (Self::BillingNotConfigured, Lang::Vi) => "Tính năng thanh toán chưa được cấu hình trên server này.".to_string(),
            (Self::BillingNotConfigured, Lang::En) => "Billing isn't configured on this server.".to_string(),
            (Self::FailedToCreateCheckoutSession, Lang::Vi) => "Không thể tạo phiên thanh toán.".to_string(),
            (Self::FailedToCreateCheckoutSession, Lang::En) => "Failed to create a checkout session.".to_string(),
        }
    }
}

pub(crate) fn error_page(lang: crate::i18n::Lang, err: OauthError) -> axum::response::Response {
    let (html_lang, back_label) = match lang {
        crate::i18n::Lang::Vi => ("vi", "Quay lại"),
        crate::i18n::Lang::En => ("en", "Back"),
    };
    Html(format!(
        r#"<!doctype html>
<html lang="{html_lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="logout" href="/me">{back_label}</a></div>
<p class="hint">{}</p>
</body></html>"#,
        crate::session::html_escape(&err.message(lang))
    ))
    .into_response()
}
