use leptos::ev::Event;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::column_mapping::{schema_from_pairs, ColumnMapping, Field};

#[derive(Clone, Serialize, Deserialize)]
pub struct CandidateData {
    pub icon: String,
    pub title: String,
    pub database_id: String,
    /// Every property as `(name, notion type)`, so the user can choose which
    /// one backs each calendar field instead of the app guessing from
    /// hardcoded English names.
    pub properties: Vec<(String, String)>,
}

impl CandidateData {
    pub fn date_properties(&self) -> Vec<&str> {
        self.properties
            .iter()
            .filter(|(_, notion_type)| notion_type == "date")
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Properties this field can legally read from, for its dropdown.
    pub fn options_for(&self, field: Field) -> Vec<&str> {
        self.properties
            .iter()
            .filter(|(_, notion_type)| field.accepts_type(notion_type))
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PickDatabasesPageData {
    pub email: String,
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

        let mut properties: Vec<(String, String)> = ds
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|props| {
                props
                    .iter()
                    .filter_map(|(name, def)| {
                        let notion_type = def.get("type")?.as_str()?;
                        Some((name.clone(), notion_type.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        properties.sort_by_key(|(name, _)| name.to_lowercase());

        candidates.push(CandidateData {
            icon,
            title,
            database_id: database_id.to_string(),
            properties,
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

/// One labelled `<select>` listing the properties a calendar field can read.
///
/// `selected` is the preselected property name — the same guess the app used
/// to apply silently, now visible and overridable. An empty value means the
/// user wants nothing mapped to this field.
#[component]
pub fn PropertySelect(
    name: String,
    label: String,
    hint: &'static str,
    options: Vec<String>,
    selected: Option<String>,
    #[prop(default = false)] required: bool,
) -> impl IntoView {
    let empty_selected = selected.is_none();
    let options = options
        .into_iter()
        .map(|option| {
            let is_selected = selected.as_deref() == Some(option.as_str());
            let label = option.clone();
            view! { <option value=option selected=is_selected>{label}</option> }
        })
        .collect_view();

    view! {
        <label class="flex flex-col gap-xs">
            <span class="text-label-md text-primary">{label}</span>
            <select
                name=name
                required=required
                class="h-[40px] px-sm border border-outline-variant rounded bg-white text-body-md text-on-surface focus:border-primary focus:outline-none"
            >
                // A required field with nothing preselected gets an empty
                // placeholder, so `required` has something invalid to reject
                // and the user has to make the choice rather than inherit a
                // default that may well be wrong.
                {(required && empty_selected).then(|| view! {
                    <option value="" selected=true disabled=true>"— choose a column —"</option>
                })}
                {(!required).then(|| view! {
                    <option value="" selected=empty_selected>"— not synced —"</option>
                })}
                {options}
            </select>
            <span class="text-on-surface-variant text-label-sm">{hint}</span>
        </label>
    }
}

/// The per-database mapping panel: which Notion property backs the date and
/// each optional calendar field.
///
/// Shown collapsed-but-present rather than conditionally rendered, so the
/// selects keep their values when a database is unticked and reticked. The
/// server only reads the mapping of databases whose checkbox was submitted.
#[component]
fn ColumnMappingFields(candidate: CandidateData, visible: RwSignal<bool>) -> impl IntoView {
    let schema = schema_from_pairs(&candidate.properties);
    let guess = ColumnMapping::guess_from_schema(&schema);
    let db_id = candidate.database_id.clone();

    let date_options: Vec<String> = candidate
        .date_properties()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    // The old code took whichever date column sorted first alphabetically,
    // which is how calendars ended up syncing "Archivierungsdatum" or
    // "Completed" instead of the deadline the user meant. When there is a
    // genuine choice, make the user make it — preselecting would quietly
    // reinstate the very bug this page exists to fix. With a single date
    // column there is nothing to decide, so keep that path frictionless.
    let date_selected = (date_options.len() == 1)
        .then(|| date_options.first().cloned())
        .flatten();

    let field_selects = Field::ALL
        .into_iter()
        .map(|field| {
            let options: Vec<String> = candidate
                .options_for(field)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            let selected = guess.get(field).map(|s| s.to_string());
            let hint = if options.is_empty() {
                "No compatible property in this database"
            } else {
                field.type_hint()
            };
            view! {
                <PropertySelect
                    name=format!("map[{}][{}]", db_id, field.key())
                    label=field.label().to_string()
                    hint=hint
                    options=options
                    selected=selected
                />
            }
        })
        .collect_view();

    view! {
        <div
            class="border-t border-outline-variant bg-surface-container-low px-md py-md"
            class:hidden=move || !visible.get()
        >
            <p class="text-on-surface-variant text-label-md mb-md">
                "Choose which Notion property backs each calendar field. You can change this later."
            </p>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-md">
                <PropertySelect
                    name=format!("date[{}]", candidate.database_id)
                    label="Date (required)".to_string()
                    hint="Rows with this property empty are not synced"
                    options=date_options
                    selected=date_selected
                    required=true
                />
                {field_selects}
            </div>
        </div>
    }
}

#[component]
pub fn PickDatabasesRoutePage() -> impl IntoView {
    let connection_id = crate::page_shell::query_param_i64("connection_id").unwrap_or_default();
    #[cfg(feature = "ssr")]
    let email = crate::page_shell::current_user_email();
    #[cfg(not(feature = "ssr"))]
    let email = String::new();
    let candidates = Resource::new(move || connection_id, list_candidates);
    view! {
        <leptos_meta::Title text="Pick a database — NotionCal"/>
        <leptos_meta::Style>{crate::page_shell::ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <Suspense fallback=|| ()>
            {move || candidates.get().map(|result| {
                let candidates = result.unwrap_or_default();
                view! {
                    <PickDatabasesPage data=PickDatabasesPageData {
                        email: email.clone(),
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
    if data.candidates.is_empty() {
        return view! {
            <div id="pick-databases-root" class="pt-[64px]">
                <crate::page_shell::HomeHeader email=data.email/>
                <main class="flex-grow flex flex-col items-center justify-center px-margin-mobile text-center">
                    <h1 class="text-h1 text-primary mb-sm">"Pick a database to sync"</h1>
                    <p class="text-on-surface-variant text-body-lg">
                        "No databases found that you've granted access to. "
                        <a class="text-secondary underline" href="/connect/notion/start" rel="external">"Grant more access on Notion"</a>
                        "."
                    </p>
                </main>
                <div class="max-w-[1280px] mx-auto w-full px-margin-desktop">
                    <crate::page_shell::PageFooter/>
                </div>
            </div>
        }
        .into_any();
    }

    let syncable_checked_signals: Vec<Option<RwSignal<bool>>> = data
        .candidates
        .iter()
        .map(|c| (!c.date_properties().is_empty()).then(|| RwSignal::new(true)))
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
        .map(|(c, sig)| match sig {
            Some(sig) => view! {
                <div class="bg-white border border-outline-variant rounded-lg card-shadow overflow-hidden">
                    <label class="group flex items-center gap-md p-md cursor-pointer hover:border-primary transition-all duration-200">
                        <input
                            type="checkbox"
                            name="db_ids"
                            value=c.database_id.clone()
                            prop:checked=move || sig.get()
                            on:change=move |ev: Event| sig.set(event_target_checked(&ev))
                            class="w-5 h-5 border-2 border-outline-variant rounded-sm text-primary focus:ring-0 focus:ring-offset-0 custom-checkbox"
                        />
                        <div class="flex items-center justify-center w-10 h-10 bg-surface-container rounded-lg text-xl">{c.icon.clone()}</div>
                        <div class="flex-grow">
                            <h3 class="text-h3 text-primary">{c.title.clone()}</h3>
                            <p class="text-on-surface-variant text-label-md flex items-center gap-xs">
                                <span class="material-symbols-outlined !text-[14px]">"tune"</span>
                                {format!("{} properties to map", c.properties.len())}
                            </p>
                        </div>
                    </label>
                    <ColumnMappingFields candidate=c visible=sig/>
                </div>
            }
            .into_any(),
            None => view! {
                <div class="flex items-center gap-md p-md bg-surface-container-low border border-outline-variant opacity-60 rounded-lg grayscale cursor-not-allowed">
                    <input type="checkbox" disabled=true class="w-5 h-5 border-2 border-outline-variant rounded-sm bg-surface-container-highest cursor-not-allowed"/>
                    <div class="flex items-center justify-center w-10 h-10 bg-surface-container-high rounded-lg text-xl">{c.icon}</div>
                    <div class="flex-grow">
                        <div class="flex items-center gap-sm">
                            <h3 class="text-h3 text-on-surface-variant">{c.title}</h3>
                            <span class="bg-error-container text-on-error-container text-[10px] px-xs py-[2px] rounded font-bold uppercase tracking-wider">"Error"</span>
                        </div>
                        <p class="text-error text-label-md flex items-center gap-xs mt-1">
                            <span class="material-symbols-outlined !text-[14px]">"warning"</span>
                            "No date property found"
                        </p>
                    </div>
                </div>
            }
            .into_any(),
        })
        .collect_view();

    view! {
        <div id="pick-databases-root" class="pt-[64px]">
            <crate::page_shell::HomeHeader email=data.email/>
            <main class="flex-grow flex flex-col pt-lg pb-32">
                <div class="max-w-[720px] mx-auto w-full px-margin-mobile md:px-0">
                    <section class="mb-xl">
                        <h1 class="text-h1 text-primary mb-sm">"Pick a database to sync"</h1>
                        <p class="text-on-surface-variant text-body-lg">"We found the following databases in your Notion workspace. Pick the one(s) you want to turn into a calendar."</p>
                    </section>
                    <form method="post" action="/connect/notion/databases">
                        <input type="hidden" name="connection_id" value=connection_id/>
                        <div class="space-y-md">{rows}</div>
                        <div class="mt-xl text-center">
                            <a class="text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/connect/notion/start" rel="external">"Don't see the database you need? Grant more access on Notion"</a>
                        </div>
                        <crate::page_shell::PageFooter/>
                        <div class="fixed bottom-0 left-0 right-0 bg-white/80 backdrop-blur-md border-t border-outline-variant py-md z-40">
                            <div class="max-w-[1280px] mx-auto px-margin-desktop flex justify-between items-center">
                                <leptos_router::components::A href="/me" attr:class="px-lg h-[40px] border border-outline-variant text-primary text-label-md rounded hover:bg-surface-container-low transition-colors flex items-center gap-sm">
                                    <span class="material-symbols-outlined !text-[18px]">"arrow_back"</span>
                                    "Back"
                                </leptos_router::components::A>
                                <button
                                    type="submit"
                                    id="continue-btn"
                                    disabled=move || checked_count.get() == 0
                                    class="px-lg h-[40px] bg-primary text-white text-label-md rounded hover:opacity-90 transition-all flex items-center gap-sm active:scale-95 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    {move || if checked_count.get() > 0 {
                                        format!("Continue with {} databases", checked_count.get())
                                    } else {
                                        "Select at least 1 database".to_string()
                                    }}
                                    <span class="material-symbols-outlined !text-[18px]">
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
