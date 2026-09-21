use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use tracing::error;

use crate::crypto::generate_token;
use crate::error_page::{error_page, OauthError};
use crate::session::find_or_create_user;
use crate::AppState;

pub(crate) const NOTION_VERSION: &str = "2025-09-03";

#[derive(Debug, Clone)]
pub struct NotionOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl NotionOAuthConfig {
    pub fn from_env(app_base_url: &str) -> Option<Self> {
        let client_id = std::env::var("NOTION_OAUTH_CLIENT_ID").ok()?;
        let client_secret = std::env::var("NOTION_OAUTH_CLIENT_SECRET").ok()?;
        Some(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{app_base_url}/oauth/notion/callback"),
        })
    }
}

struct ConnectNotionLabels {
    title: &'static str,
    heading: &'static str,
    body: &'static str,
    connect_cta: &'static str,
    bullet_read_write: &'static str,
    bullet_disconnect: &'static str,
    bullet_no_sharing: &'static str,
    privacy_link: &'static str,
    terms_link: &'static str,
}

const CONNECT_NOTION_LABELS_VI: ConnectNotionLabels = ConnectNotionLabels {
    title: "Kết nối Notion",
    heading: "Kết nối không gian làm việc Notion của bạn",
    body: "Chúng tôi cần quyền truy cập vào không gian làm việc Notion của bạn để tìm và đồng bộ hóa các cơ sở dữ liệu bạn chọn. Bạn sẽ chọn chính xác trang nào cần chia sẻ ở bước tiếp theo trên Notion.",
    connect_cta: "Kết nối với Notion",
    bullet_read_write: "Chỉ đọc và ghi vào các trang bạn cho phép",
    bullet_disconnect: "Có thể ngắt kết nối bất cứ lúc nào",
    bullet_no_sharing: "Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba",
    privacy_link: "Chính sách bảo mật",
    terms_link: "Điều khoản dịch vụ",
};

const CONNECT_NOTION_LABELS_EN: ConnectNotionLabels = ConnectNotionLabels {
    title: "Connect Notion",
    heading: "Connect your Notion workspace",
    body: "We need access to your Notion workspace to find and sync the databases you choose. You'll pick exactly which pages to share in the next step, on Notion.",
    connect_cta: "Connect to Notion",
    bullet_read_write: "Only reads and writes the pages you allow",
    bullet_disconnect: "Disconnect anytime",
    bullet_no_sharing: "Never shares your data with third parties",
    privacy_link: "Privacy Policy",
    terms_link: "Terms of Service",
};

pub async fn connect_notion_page(
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    request: axum::extract::Request,
) -> axum::response::Response {
    let email = claims.email().map(|e| e.as_str()).unwrap_or("");
    let l = match lang {
        crate::i18n::Lang::Vi => &CONNECT_NOTION_LABELS_VI,
        crate::i18n::Lang::En => &CONNECT_NOTION_LABELS_EN,
    };
    let data = app::connect_notion::ConnectNotionPageData {
        html_lang: lang.code().to_string(),
        title: l.title.to_string(),
        top_nav_html: crate::i18n::top_nav_html(email, lang, "/connect/notion"),
        heading: l.heading.to_string(),
        body: l.body.to_string(),
        connect_cta: l.connect_cta.to_string(),
        bullet_read_write: l.bullet_read_write.to_string(),
        bullet_disconnect: l.bullet_disconnect.to_string(),
        bullet_no_sharing: l.bullet_no_sharing.to_string(),
        privacy_link: l.privacy_link.to_string(),
        terms_link: l.terms_link.to_string(),
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::connect_notion::ConnectNotionShell data=data/> }
    });
    handler(request).await
}

pub async fn connect_notion_start(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    lang: crate::i18n::Lang,
) -> impl IntoResponse {
    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(lang, OauthError::NotionNotConfigured);
    };

    let oauth_state = generate_token(32);
    if let Err(e) = session.insert("notion_oauth_state", &oauth_state).await {
        error!("failed to store notion oauth state: {}", e);
        return error_page(lang, OauthError::TryAgain);
    }

    let mut url = url::Url::parse(&format!("{}/v1/oauth/authorize", state.notion_api_base_url))
        .expect("valid notion_api_base_url");
    url.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("owner", "user")
        .append_pair("redirect_uri", &cfg.redirect_uri)
        .append_pair("state", &oauth_state);

    Redirect::to(url.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn notion_oauth_callback(
    State(state): State<AppState>,
    session: tower_sessions::Session,
    claims: OidcClaims<EmptyAdditionalClaims>,
    lang: crate::i18n::Lang,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    if let Some(err) = params.error {
        return error_page(lang, OauthError::NotionDenied(err));
    }
    let Some(code) = params.code else {
        return error_page(lang, OauthError::MissingAuthCode);
    };

    let expected_state: Option<String> = session.get("notion_oauth_state").await.unwrap_or(None);
    let _ = session.remove::<String>("notion_oauth_state").await;
    if expected_state.is_none() || expected_state.as_deref() != params.state.as_deref() {
        return error_page(lang, OauthError::InvalidSession);
    }

    let Some(cfg) = state.notion_oauth.clone() else {
        return error_page(lang, OauthError::NotionNotConfigured);
    };

    let resp = match state
        .client
        .post(format!("{}/v1/oauth/token", state.notion_api_base_url))
        .basic_auth(&cfg.client_id, Some(&cfg.client_secret))
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": cfg.redirect_uri,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("notion token exchange request failed: {}", e);
            return error_page(lang, OauthError::CantReachNotion);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        error!("notion token exchange failed {}: {}", status, txt);
        return error_page(lang, OauthError::NotionRejectedToken);
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to parse notion token response: {}", e);
            return error_page(lang, OauthError::InvalidNotionResponse);
        }
    };

    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_id = body
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let workspace_name = body
        .get("workspace_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let bot_id = body
        .get("bot_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if access_token.is_empty() || workspace_id.is_empty() {
        return error_page(lang, OauthError::NotionResponseMissingFields);
    }

    let sub = claims.subject().as_str();
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();
    let user_id = match find_or_create_user(&state, sub, &email, lang).await {
        Ok(id) => id,
        Err(e) => {
            error!("failed to find_or_create_user: {}", e);
            return error_page(lang, OauthError::Generic);
        }
    };

    let connection_id: i64 = match sqlx::query_scalar(
        "INSERT INTO notion_connections (user_id, notion_access_token, workspace_id, workspace_name, bot_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, workspace_id) DO UPDATE SET
             notion_access_token = EXCLUDED.notion_access_token,
             workspace_name = EXCLUDED.workspace_name,
             bot_id = EXCLUDED.bot_id
         RETURNING id",
    )
    .bind(user_id)
    .bind(&access_token)
    .bind(&workspace_id)
    .bind(&workspace_name)
    .bind(&bot_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!("failed to upsert notion_connection: {}", e);
            return error_page(lang, OauthError::FailedToSaveConnection);
        }
    };

    Redirect::to(&format!(
        "/connect/notion/databases?connection_id={connection_id}"
    ))
    .into_response()
}
