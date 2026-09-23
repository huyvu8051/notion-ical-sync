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
    FailedToDeleteCalendar,
    BillingNotConfigured,
    FailedToCreateCheckoutSession,
}

impl OauthError {
    fn message(&self) -> String {
        match self {
            Self::NotionNotConfigured => "Notion OAuth isn't configured on this server.".to_string(),
            Self::TryAgain => "Something went wrong, please try again.".to_string(),
            Self::NotionDenied(reason) => format!("Notion denied the authorization request: {reason}"),
            Self::MissingAuthCode => "Missing authorization code from Notion.".to_string(),
            Self::InvalidSession => "Invalid auth session, please try again.".to_string(),
            Self::CantReachNotion => "Couldn't connect to Notion.".to_string(),
            Self::NotionRejectedToken => "Notion rejected the token exchange request.".to_string(),
            Self::InvalidNotionResponse => "Invalid response from Notion.".to_string(),
            Self::NotionResponseMissingFields => "Notion's response is missing access_token or workspace_id.".to_string(),
            Self::Generic => "Something went wrong.".to_string(),
            Self::FailedToSaveConnection => "Failed to save the Notion connection.".to_string(),
            Self::ConnectionNotFound => "This Notion connection wasn't found.".to_string(),
            Self::FailedToListDatabases => "Failed to list databases from Notion.".to_string(),
            Self::InvalidRequest => "Invalid request.".to_string(),
            Self::CalendarNotFound => "This calendar wasn't found.".to_string(),
            Self::FailedToDeleteCalendar => "Failed to delete this calendar.".to_string(),
            Self::BillingNotConfigured => "Billing isn't configured on this server.".to_string(),
            Self::FailedToCreateCheckoutSession => "Failed to create a checkout session.".to_string(),
        }
    }
}

pub(crate) fn error_page(err: OauthError) -> axum::response::Response {
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{AUTH_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="logout" href="/me">Back</a></div>
<p class="hint">{}</p>
</body></html>"#,
        crate::session::html_escape(&err.message())
    ))
    .into_response()
}
