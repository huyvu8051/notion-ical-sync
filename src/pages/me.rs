use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use tracing::error;

use crate::error_page::{error_page, OauthError};
use crate::session::owned_calendar_or_error;
use crate::AppState;

pub async fn delete_calendar(
    State(state): State<AppState>,
    claims: OidcClaims<EmptyAdditionalClaims>,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let cal = match owned_calendar_or_error(&state, &claims, &public_id).await {
        Ok(cal) => cal,
        Err(resp) => return resp,
    };

    if let Err(e) = sqlx::query("DELETE FROM calendars WHERE id = $1")
        .bind(cal.id)
        .execute(&state.db)
        .await
    {
        error!("failed to delete calendar {}: {}", cal.id, e);
        return error_page(OauthError::FailedToDeleteCalendar);
    }

    let still_referenced: i64 =
        sqlx::query_scalar("SELECT count(*) FROM calendars WHERE database_id = $1")
            .bind(&cal.database_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(1);
    if still_referenced == 0 {
        state.cache.write().await.remove(&cal.database_id);
    }

    Redirect::to("/me").into_response()
}
