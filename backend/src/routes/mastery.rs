use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetBody {
    pub question_id: i32,
}

pub async fn reset(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ResetBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    sqlx::query(
        "UPDATE srs_cards
         SET state = 'learning', interval_days = 0, ease = 2.5, reps = 0, step_index = 0,
             lapses = 0, suspended = false, due = now()
         WHERE user_id = $1 AND question_id = $2",
    )
    .bind(user_id)
    .bind(body.question_id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({ "success": true })))
}
