use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let row: (Option<String>,) =
        sqlx::query_as("SELECT game_type_filters FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let filters: Vec<Value> = match row.0 {
        Some(s) if !s.is_empty() => serde_json::from_str(&s).unwrap_or_default(),
        _ => vec![],
    };

    Ok(Json(json!({ "gameTypeFilters": filters })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreferencesBody {
    pub game_type_filters: Vec<String>,
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdatePreferencesBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let filters_json = serde_json::to_string(&body.game_type_filters)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query("UPDATE users SET game_type_filters = $1 WHERE id = $2")
        .bind(&filters_json)
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "success": true })))
}
