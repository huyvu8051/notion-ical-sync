//! Change which Notion property backs each field of an existing calendar.
//!
//! Until this page existed a calendar row was written once at creation and
//! never updated, so a wrong guess — most often the date column, picked
//! alphabetically — could only be fixed by deleting the calendar and
//! reconnecting it, which changes the CalDAV URL the user already subscribed
//! to on their devices.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::column_mapping::{schema_from_pairs, ColumnMapping, Field};
use crate::pick_databases::PropertySelect;

#[derive(Clone, Serialize, Deserialize)]
pub struct EditMappingPageData {
    pub email: String,
    pub public_id: String,
    pub calendar_name: String,
    pub date_property: String,
    pub mapping: ColumnMapping,
    /// Every property of the backing data source as `(name, notion type)`.
    pub properties: Vec<(String, String)>,
    /// Set when the schema could not be fetched from Notion — the form is not
    /// safe to render then, because every dropdown would look empty and
    /// saving would wipe a working mapping.
    pub load_error: Option<String>,
}

impl EditMappingPageData {
    fn options_for(&self, field: Field) -> Vec<String> {
        self.properties
            .iter()
            .filter(|(_, notion_type)| field.accepts_type(notion_type))
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn date_options(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|(_, notion_type)| notion_type == "date")
            .map(|(name, _)| name.clone())
            .collect()
    }
}

/// `(display_name, data_source_id, date_property, column_mapping, token)`.
///
/// A tuple rather than a `FromRow` struct because this crate's sqlx is built
/// without the derive feature — the same shape every other server fn here uses.
#[cfg(feature = "ssr")]
type CalendarMappingRow = (
    String,
    String,
    String,
    sqlx::types::Json<ColumnMapping>,
    String,
);

#[cfg(feature = "ssr")]
async fn owned_calendar(
    pool: &sqlx::PgPool,
    claims: &axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims>,
    public_id: &str,
) -> Result<CalendarMappingRow, ServerFnError> {
    sqlx::query_as::<_, CalendarMappingRow>(
        "SELECT c.display_name, c.data_source_id, c.date_property, c.column_mapping, \
         nc.notion_access_token \
         FROM calendars c JOIN notion_connections nc ON nc.id = c.notion_connection_id \
         WHERE c.public_id = $1 AND c.user_id = (SELECT id FROM users WHERE keycloak_sub = $2)",
    )
    .bind(public_id)
    .bind(claims.subject().as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("failed to look up calendar: {e}")))?
    .ok_or_else(|| ServerFnError::new("calendar not found"))
}

/// Read the live property list of a data source, so the dropdowns offer what
/// the database has right now rather than what it had at connect time.
#[cfg(feature = "ssr")]
async fn fetch_properties(
    client: &reqwest::Client,
    api_base_url: &str,
    token: &str,
    data_source_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let url = format!("{api_base_url}/v1/data_sources/{data_source_id}");
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .header("Notion-Version", "2025-09-03")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::error!(%url, %status, %body, "<- Notion API error response (edit mapping schema)");
        return Err(format!("Notion error {status}: {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;
    let mut properties: Vec<(String, String)> = body
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
    Ok(properties)
}

#[server]
async fn load_mapping_data(public_id: String) -> Result<EditMappingPageData, ServerFnError> {
    let claims: axum_oidc::OidcClaims<axum_oidc::EmptyAdditionalClaims> =
        leptos_axum::extract().await?;
    let pool = use_context::<sqlx::PgPool>()
        .ok_or_else(|| ServerFnError::new("missing db pool context"))?;
    let client = use_context::<reqwest::Client>()
        .ok_or_else(|| ServerFnError::new("missing http client context"))?;
    let api_base_url = use_context::<crate::page_shell::NotionApiBaseUrl>()
        .ok_or_else(|| ServerFnError::new("missing notion api base url context"))?
        .0;

    let (calendar_name, data_source_id, date_property, mapping, token) =
        owned_calendar(&pool, &claims, &public_id).await?;
    let mapping = mapping.0;
    let email = claims.email().map(|e| e.as_str()).unwrap_or("").to_string();

    let (properties, load_error) =
        match fetch_properties(&client, &api_base_url, &token, &data_source_id).await {
            Ok(properties) => (properties, None),
            Err(e) => (Vec::new(), Some(e)),
        };

    tracing::info!(
        public_id = %public_id,
        calendar = %calendar_name,
        property_count = properties.len(),
        configured = mapping.configured,
        "loaded column mapping editor"
    );

    Ok(EditMappingPageData {
        email,
        public_id,
        calendar_name,
        date_property,
        mapping,
        properties,
        load_error,
    })
}

#[component]
pub fn EditMappingRoutePage() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let public_id = move || params.with(|p| p.get("public_id").unwrap_or_default());
    let data = Resource::new(public_id, load_mapping_data);
    view! {
        <leptos_meta::Title text="Column mapping — NotionCal"/>
        <leptos_meta::Style>{crate::page_shell::ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <Suspense fallback=|| ()>
            {move || data.get().map(|result| match result {
                Ok(data) => view! { <EditMappingPage data=data/> }.into_any(),
                Err(e) => view! {
                    <main class="max-w-[720px] mx-auto px-margin-mobile pt-[96px]">
                        <h1 class="text-h1 text-primary mb-sm">"Column mapping"</h1>
                        <p class="text-error text-body-lg">{format!("Could not load this calendar: {e}")}</p>
                        <a class="text-secondary underline" href="/me">"Back to your calendars"</a>
                    </main>
                }
                .into_any(),
            })}
        </Suspense>
    }
}

#[component]
pub fn EditMappingPage(data: EditMappingPageData) -> impl IntoView {
    let action = format!("/me/calendars/{}/mapping", data.public_id);

    if let Some(error) = data.load_error.clone() {
        return view! {
            <div id="edit-mapping-root" class="pt-[64px]">
                <crate::page_shell::HomeHeader email=data.email/>
                <main class="max-w-[720px] mx-auto w-full px-margin-mobile pt-lg">
                    <h1 class="text-h1 text-primary mb-sm">{data.calendar_name}</h1>
                    <p class="text-error text-body-lg mb-md">
                        "Could not read this database from Notion, so the columns cannot be listed."
                    </p>
                    <pre class="bg-surface-container-low border border-outline-variant rounded p-md text-label-sm overflow-x-auto">{error}</pre>
                    <p class="text-on-surface-variant text-body-md mt-md">
                        "This usually means the database is no longer shared with the integration, or it was deleted. "
                        <a class="text-secondary underline" href="/connect/notion/start" rel="external">"Re-grant access on Notion"</a>
                        " and try again."
                    </p>
                </main>
            </div>
        }
        .into_any();
    }

    let date_options = data.date_options();
    let date_selected = Some(data.date_property.clone());
    let field_selects = Field::ALL
        .into_iter()
        .map(|field| {
            let options = data.options_for(field);
            let hint = if options.is_empty() {
                "No compatible property in this database"
            } else {
                field.type_hint()
            };
            // An unconfigured calendar shows the guess it has been running on,
            // so the user sees what it is doing today before changing it.
            let selected = if data.mapping.configured {
                data.mapping.get(field).map(|s| s.to_string())
            } else {
                ColumnMapping::guess_from_schema(&schema_from_pairs(&data.properties))
                    .get(field)
                    .map(|s| s.to_string())
            };
            view! {
                <PropertySelect
                    name=format!("map[{}]", field.key())
                    label=field.label().to_string()
                    hint=hint
                    options=options
                    selected=selected
                />
            }
        })
        .collect_view();

    view! {
        <div id="edit-mapping-root" class="pt-[64px]">
            <crate::page_shell::HomeHeader email=data.email/>
            <main class="flex-grow flex flex-col pt-lg pb-32">
                <div class="max-w-[720px] mx-auto w-full px-margin-mobile md:px-0">
                    <section class="mb-xl">
                        <h1 class="text-h1 text-primary mb-sm">{data.calendar_name}</h1>
                        <p class="text-on-surface-variant text-body-lg">
                            "Choose which Notion property backs each calendar field. Saving re-syncs this calendar immediately."
                        </p>
                    </section>
                    <form method="post" action=action>
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-md">
                            <PropertySelect
                                name="date".to_string()
                                label="Date (required)".to_string()
                                hint="Rows with this property empty are not synced"
                                options=date_options
                                selected=date_selected
                                required=true
                            />
                            {field_selects}
                        </div>
                        <div class="fixed bottom-0 left-0 right-0 bg-white/80 backdrop-blur-md border-t border-outline-variant py-md z-40">
                            <div class="max-w-[1280px] mx-auto px-margin-desktop flex justify-between items-center">
                                <a href="/me" class="px-lg h-[40px] border border-outline-variant text-primary text-label-md rounded hover:bg-surface-container-low transition-colors flex items-center gap-sm">
                                    <span class="material-symbols-outlined !text-[18px]">"arrow_back"</span>
                                    "Cancel"
                                </a>
                                <button
                                    type="submit"
                                    class="px-lg h-[40px] bg-primary text-white text-label-md rounded hover:opacity-90 transition-all flex items-center gap-sm active:scale-95 shadow-sm"
                                >
                                    "Save and re-sync"
                                    <span class="material-symbols-outlined !text-[18px]">"sync"</span>
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
