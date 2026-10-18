//! One-shot backfill of `calendars.column_mapping` for calendars created
//! before the mapping UI existed.
//!
//! It writes exactly the guesses those calendars are already running on, so
//! behaviour does not change. The point is to turn an invisible heuristic into
//! stored data the user can see and correct on the mapping page.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use tracing::{error, info, warn};

use app::column_mapping::{ColumnMapping, Field};

use crate::AppState;

#[derive(serde::Serialize)]
pub struct BackfillReport {
    examined: usize,
    written: usize,
    skipped_already_configured: usize,
    /// Calendars whose data source could not be read — unshared, deleted, or
    /// the data source id has rotated. Left untouched for a human to look at.
    unreadable: Vec<String>,
    /// Calendars whose stored date property no longer exists in the schema.
    /// These are the ones actively producing empty calendars.
    broken_date_property: Vec<String>,
}

/// `?dry_run=true` reports what would change without writing anything.
#[derive(serde::Deserialize, Default)]
pub struct BackfillQuery {
    #[serde(default)]
    dry_run: bool,
}

pub async fn backfill_column_mappings(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<BackfillQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(expected_secret) = state.admin_secret.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let provided = headers.get("X-Admin-Secret").and_then(|v| v.to_str().ok());
    if provided != Some(expected_secret) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let dry_run = query.dry_run;

    let calendars = state.all_calendars().await;
    let mut report = BackfillReport {
        examined: calendars.len(),
        written: 0,
        skipped_already_configured: 0,
        unreadable: Vec::new(),
        broken_date_property: Vec::new(),
    };
    info!(
        calendars = report.examined,
        dry_run, "column mapping backfill started"
    );

    for cal in calendars {
        if cal.column_mapping.configured {
            report.skipped_already_configured += 1;
            continue;
        }

        let schema = state
            .get_data_source_properties(&cal.data_source_id, &cal.notion_access_token)
            .await;
        if schema.is_empty() {
            warn!(
                calendar_id = cal.id,
                calendar = %cal.display_name,
                data_source_id = %cal.data_source_id,
                "backfill: could not read data source schema, leaving mapping unset"
            );
            report.unreadable.push(cal.display_name.clone());
            continue;
        }

        if schema
            .get(&cal.date_property.to_lowercase())
            .is_none_or(|(_, notion_type)| notion_type != "date")
        {
            warn!(
                calendar_id = cal.id,
                calendar = %cal.display_name,
                date_property = %cal.date_property,
                "backfill: stored date property is gone or no longer a date"
            );
            report.broken_date_property.push(format!(
                "{} (date_property={})",
                cal.display_name, cal.date_property
            ));
        }

        let mapping = ColumnMapping::guess_from_schema(&schema);
        info!(
            calendar_id = cal.id,
            calendar = %cal.display_name,
            title_property = mapping.name_for(Field::Title).unwrap_or("<none>"),
            mapped = Field::ALL.iter().filter(|f| mapping.get(**f).is_some()).count(),
            dry_run,
            "backfill: resolved mapping"
        );

        if dry_run {
            report.written += 1;
            continue;
        }

        match sqlx::query("UPDATE calendars SET column_mapping = $1 WHERE id = $2")
            .bind(sqlx::types::Json(&mapping))
            .bind(cal.id)
            .execute(&state.db)
            .await
        {
            Ok(_) => report.written += 1,
            Err(e) => error!(
                calendar_id = cal.id,
                error = %e,
                "backfill: failed to write column mapping"
            ),
        }
    }

    info!(
        examined = report.examined,
        written = report.written,
        skipped = report.skipped_already_configured,
        unreadable = report.unreadable.len(),
        broken_date_property = report.broken_date_property.len(),
        dry_run,
        "column mapping backfill finished"
    );

    Json(report).into_response()
}
