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

    Redirect::to("/me").into_response()
}
