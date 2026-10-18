use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use tracing::{error, info, warn};

use app::column_mapping::{ColumnMapping, Field};

use crate::error_page::{error_page, OauthError};
use crate::session::owned_calendar_or_error;
use crate::AppState;

struct MappingForm {
    date_property: Option<String>,
    mapping: ColumnMapping,
}

impl MappingForm {
    fn parse(body: &str) -> Self {
        let mut date_property = None;
        // The form shows every field, so anything the user left on
        // "— not synced —" is a deliberate none rather than a missing value.
        let mut mapping = ColumnMapping {
            configured: true,
            ..Default::default()
        };

        for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
            match key.as_ref() {
                "date" if !value.is_empty() => date_property = Some(value.into_owned()),
                k if k.starts_with("map[") => {
                    let Some(field_key) = k
                        .strip_prefix("map[")
                        .and_then(|rest| rest.strip_suffix(']'))
                    else {
                        warn!(key = k, "ignoring malformed mapping form field");
                        continue;
                    };
                    let Some(field) = Field::from_key(field_key) else {
                        warn!(key = k, field_key, "ignoring unknown mapping field");
                        continue;
                    };
                    mapping.set(field, Some(value.into_owned()));
                }
                _ => {}
            }
        }

        Self {
            date_property,
            mapping,
        }
    }
}

pub async fn update_mapping(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    let body = String::from_utf8_lossy(&body);
    let form = MappingForm::parse(&body);

    // Validate against the live schema rather than trusting the posted names:
    // the database may have changed since the page was rendered, and a stale
    // tab must not be able to write a mapping that cannot resolve.
    let schema = state
        .get_data_source_properties(&cal.data_source_id, &cal.notion_access_token)
        .await;
    if schema.is_empty() {
        error!(
            calendar_id = cal.id,
            data_source_id = %cal.data_source_id,
            "refusing to save mapping: could not read the data source schema"
        );
        return error_page(OauthError::Generic);
    }

    let date_property = match form.date_property {
        Some(chosen)
            if schema
                .get(&chosen.to_lowercase())
                .is_some_and(|(_, notion_type)| notion_type == "date") =>
        {
            chosen
        }
        other => {
            warn!(
                calendar_id = cal.id,
                requested = other.as_deref().unwrap_or("<none>"),
                current = %cal.date_property,
                "rejected date property, keeping the current one"
            );
            cal.date_property.clone()
        }
    };

    let mut mapping = form.mapping;
    mapping.retain_valid(&schema);

    info!(
        calendar_id = cal.id,
        calendar = %cal.display_name,
        date_property = %date_property,
        title_property = mapping.name_for(Field::Title).unwrap_or("<none>"),
        mapped = Field::ALL.iter().filter(|f| mapping.get(**f).is_some()).count(),
        "saving column mapping"
    );

    if let Err(e) =
        sqlx::query("UPDATE calendars SET date_property = $1, column_mapping = $2 WHERE id = $3")
            .bind(&date_property)
            .bind(sqlx::types::Json(&mapping))
            .bind(cal.id)
            .execute(&state.db)
            .await
    {
        error!(calendar_id = cal.id, error = %e, "failed to save column mapping");
        return error_page(OauthError::Generic);
    }

    // Re-sync now so the user sees the effect of their change immediately
    // rather than at the next ten-minute tick.
    state.refresh_by_data_source(&cal.data_source_id).await;

    Redirect::to("/me").into_response()
}
