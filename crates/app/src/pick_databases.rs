use leptos::ev::Event;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct CandidateData {
    pub icon: String,
    pub title: String,
    pub database_id: String,
    pub date_property: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PickDatabasesPageData {
    pub top_nav_html: String,
    pub connection_id: i64,
    pub candidates: Vec<CandidateData>,
}

#[cfg(feature = "ssr")]
const NOTION_VERSION: &str = "2025-09-03";

#[cfg(feature = "ssr")]
async fn connection_token_for_user(
    pool: &sqlx::PgPool,
    connection_id: i64,
    keycloak_sub: &str,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT nc.notion_access_token FROM notion_connections nc \
         JOIN users u ON u.id = nc.user_id \
         WHERE nc.id = $1 AND u.keycloak_sub = $2",
    )
    .bind(connection_id)
    .bind(keycloak_sub)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

#[cfg(feature = "ssr")]
async fn fetch_syncable_databases(
    client: &reqwest::Client,
    api_base_url: &str,
    token: &str,
) -> Result<Vec<CandidateData>, String> {
    let resp = client
        .post(format!("{api_base_url}/v1/search"))
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .json(&serde_json::json!({ "filter": { "value": "data_source", "property": "object" } }))
        .send()
        .await
        .map_err(|e| format!("search request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Notion search error {status}: {txt}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;
    let results = body
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    let mut candidates = Vec::new();
    for ds in results {
        let Some(database_id) = ds
            .get("parent")
            .and_then(|p| p.get("database_id"))
            .and_then(|id| id.as_str())
        else {
            continue;
        };

        let title = ds
            .get("title")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("plain_text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)")
            .to_string();
        let icon = ds
            .get("icon")
            .and_then(|icon| icon.get("emoji"))
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "📄".to_string());

        let date_property = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .and_then(|props| {
                props
                    .iter()
                    .find(|(_, def)| def.get("type").and_then(|t| t.as_str()) == Some("date"))
                    .map(|(name, _)| name.clone())
            });

        candidates.push(CandidateData {
            icon,
            title,
            database_id: database_id.to_string(),
            date_property,
        });
    }

    Ok(candidates)
}

#[server]
async fn list_candidates(connection_id: i64) -> Result<Vec<CandidateData>, ServerFnError> {
    let claims: axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims> =
        leptos_axum::extract().await?;
    let pool = use_context::<sqlx::PgPool>()
        .ok_or_else(|| ServerFnError::new("missing db pool context"))?;
    let client = use_context::<reqwest::Client>()
        .ok_or_else(|| ServerFnError::new("missing http client context"))?;
    let api_base_url = use_context::<crate::page_shell::NotionApiBaseUrl>()
        .ok_or_else(|| ServerFnError::new("missing notion api base url context"))?
        .0;

    let Some(access_token) =
        connection_token_for_user(&pool, connection_id, claims.subject().as_str()).await
    else {
        return Err(ServerFnError::new("connection not found"));
    };

    fetch_syncable_databases(&client, &api_base_url, &access_token)
        .await
        .map_err(ServerFnError::new)
}

#[component]
pub fn PickDatabasesRoutePage() -> impl IntoView {
    let connection_id = crate::page_shell::query_param_i64("connection_id").unwrap_or_default();
    #[cfg(feature = "ssr")]
    let email = crate::page_shell::current_user_email();
    #[cfg(not(feature = "ssr"))]
    let email = String::new();
    let top_nav_html = crate::page_shell::top_nav_html(
        &email,
        crate::page_shell::detect_lang(),
        "/connect/notion/databases",
    );
    let candidates = Resource::new(move || connection_id, list_candidates);
    view! {
        <leptos_meta::Title text="Chọn cơ sở dữ liệu — NotionCal"/>
        <leptos_meta::Link rel="stylesheet" href="/assets/style-auth-a.css"/>
        <leptos_meta::Link href=crate::page_shell::GOOGLE_FONTS_HREF rel="stylesheet"/>
        <leptos_meta::Style>{crate::page_shell::ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <Suspense fallback=|| ()>
            {move || candidates.get().map(|result| {
                let candidates = result.unwrap_or_default();
                view! {
                    <PickDatabasesPage data=PickDatabasesPageData {
                        top_nav_html: top_nav_html.clone(),
                        connection_id,
                        candidates,
                    }/>
                }
            })}
        </Suspense>
    }
}

#[component]
pub fn PickDatabasesPage(data: PickDatabasesPageData) -> impl IntoView {
    let top_nav_html = data.top_nav_html.clone();

    if data.candidates.is_empty() {
        return view! {
            <div id="pick-databases-root">
                <div inner_html=top_nav_html></div>
                <main class="flex-grow flex flex-col items-center justify-center px-margin-mobile text-center">
                    <h1 class="text-h1 text-primary mb-sm">"Chọn cơ sở dữ liệu để đồng bộ"</h1>
                    <p class="text-on-surface-variant text-body-lg">
                        "Không tìm thấy cơ sở dữ liệu nào bạn đã cấp quyền. "
                        <a class="text-secondary underline" href="/connect/notion/start">"Cấp thêm quyền truy cập trên Notion"</a>
                        "."
                    </p>
                </main>
            </div>
        }
        .into_any();
    }

    let syncable_checked_signals: Vec<Option<RwSignal<bool>>> = data
        .candidates
        .iter()
        .map(|c| c.date_property.is_some().then(|| RwSignal::new(true)))
        .collect();

    let checked_count = {
        let syncable_checked_signals = syncable_checked_signals.clone();
        Memo::new(move |_| {
            syncable_checked_signals
                .iter()
                .filter(|s| s.map(|s| s.get()).unwrap_or(false))
                .count()
        })
    };

    let connection_id = data.connection_id.to_string();

    let rows = data
        .candidates
        .into_iter()
        .zip(syncable_checked_signals)
        .map(|(c, sig)| match (c.date_property, sig) {
            (Some(date_prop), Some(sig)) => view! {
                <label class="group flex items-center gap-md p-md bg-white border border-outline-variant rounded-lg cursor-pointer hover:border-primary transition-all duration-200 card-shadow">
                    <input
                        type="checkbox"
                        name="db_ids"
                        value=c.database_id
                        prop:checked=move || sig.get()
                        on:change=move |ev: Event| sig.set(event_target_checked(&ev))
                        class="w-5 h-5 border-2 border-outline-variant rounded-sm text-primary focus:ring-0 focus:ring-offset-0 custom-checkbox"
                    />
                    <div class="flex items-center justify-center w-10 h-10 bg-surface-container rounded-lg text-xl">{c.icon}</div>
                    <div class="flex-grow">
                        <h3 class="text-h3 text-primary">{c.title}</h3>
                        <p class="text-on-surface-variant text-label-md flex items-center gap-xs">
                            <span class="material-symbols-outlined text-[14px]">"calendar_today"</span>
                            {format!("Có thuộc tính ngày: {date_prop}")}
                        </p>
                    </div>
                </label>
            }
            .into_any(),
            (None, _) => view! {
                <div class="flex items-center gap-md p-md bg-surface-container-low border border-outline-variant opacity-60 rounded-lg grayscale cursor-not-allowed">
                    <input type="checkbox" disabled=true class="w-5 h-5 border-2 border-outline-variant rounded-sm bg-surface-container-highest cursor-not-allowed"/>
                    <div class="flex items-center justify-center w-10 h-10 bg-surface-container-high rounded-lg text-xl">{c.icon}</div>
                    <div class="flex-grow">
                        <div class="flex items-center gap-sm">
                            <h3 class="text-h3 text-on-surface-variant">{c.title}</h3>
                            <span class="bg-error-container text-on-error-container text-[10px] px-xs py-[2px] rounded font-bold uppercase tracking-wider">"Lỗi"</span>
                        </div>
                        <p class="text-error text-label-md flex items-center gap-xs mt-1">
                            <span class="material-symbols-outlined text-[14px]">"warning"</span>
                            "Không tìm thấy thuộc tính ngày"
                        </p>
                    </div>
                </div>
            }
            .into_any(),
            (Some(_), None) => unreachable!("a syncable candidate always gets a signal"),
        })
        .collect_view();

    view! {
        <div id="pick-databases-root">
            <div inner_html=top_nav_html></div>
            <main class="flex-grow flex flex-col pt-lg pb-32">
                <div class="max-w-[720px] mx-auto w-full px-margin-mobile md:px-0">
                    <section class="mb-xl">
                        <h1 class="text-h1 text-primary mb-sm">"Chọn cơ sở dữ liệu để đồng bộ"</h1>
                        <p class="text-on-surface-variant text-body-lg">"Chúng tôi đã tìm thấy các cơ sở dữ liệu sau trong không gian làm việc Notion của bạn. Chọn (các) cơ sở dữ liệu bạn muốn biến thành lịch."</p>
                    </section>
                    <form method="post" action="/connect/notion/databases">
                        <input type="hidden" name="connection_id" value=connection_id/>
                        <div class="space-y-md">{rows}</div>
                        <div class="mt-xl text-center">
                            <a class="text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/connect/notion/start">"Không thấy cơ sở dữ liệu bạn cần? Cấp thêm quyền truy cập trên Notion"</a>
                        </div>
                        <div class="fixed bottom-0 left-0 right-0 bg-white/80 backdrop-blur-md border-t border-outline-variant py-md z-40">
                            <div class="max-w-[1280px] mx-auto px-margin-desktop flex justify-between items-center">
                                <a class="px-lg h-[40px] border border-outline-variant text-primary text-label-md rounded hover:bg-surface-container-low transition-colors flex items-center gap-sm" href="/me">
                                    <span class="material-symbols-outlined text-[18px]">"arrow_back"</span>
                                    "Quay lại"
                                </a>
                                <button
                                    type="submit"
                                    id="continue-btn"
                                    disabled=move || checked_count.get() == 0
                                    class="px-lg h-[40px] bg-primary text-white text-label-md rounded hover:opacity-90 transition-all flex items-center gap-sm active:scale-95 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    {move || if checked_count.get() > 0 {
                                        format!("Tiếp tục với {} cơ sở dữ liệu", checked_count.get())
                                    } else {
                                        "Chọn ít nhất 1 cơ sở dữ liệu".to_string()
                                    }}
                                    <span class="material-symbols-outlined text-[18px]">
                                        {move || if checked_count.get() > 0 { "arrow_forward" } else { "error" }}
                                    </span>
                                </button>
                            </div>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
    .into_any()
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_data() -> PickDatabasesPageData {
        PickDatabasesPageData {
            top_nav_html: "<header>nav</header>".to_string(),
            connection_id: 42,
            candidates: vec![
                CandidateData {
                    icon: "📄".to_string(),
                    title: "Work <Tasks>".to_string(),
                    database_id: "db-1".to_string(),
                    date_property: Some("Due date".to_string()),
                },
                CandidateData {
                    icon: "🗒".to_string(),
                    title: "No date property".to_string(),
                    database_id: "db-2".to_string(),
                    date_property: None,
                },
            ],
        }
    }

    #[test]
    fn renders_candidates_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <PickDatabasesPage data=sample_data()/> }.to_html());
        assert!(html.contains("db-1"));
        assert!(html.contains("Due date"));
        assert!(html.contains("Không tìm thấy thuộc tính ngày"));
        assert!(html.contains("&lt;Tasks&gt;") || html.contains("Work"));
    }

    #[test]
    fn renders_empty_state_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let data = PickDatabasesPageData {
            top_nav_html: "<header>nav</header>".to_string(),
            connection_id: 1,
            candidates: vec![],
        };
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <PickDatabasesPage data=data/> }.to_html());
        assert!(html.contains("Không tìm thấy cơ sở dữ liệu"));
    }
}
