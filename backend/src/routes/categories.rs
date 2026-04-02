use axum::{extract::State, Json};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, FromRow)]
struct CategoryRow {
    pub name: Option<String>,
    pub count: i64,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<CategoryRow> = sqlx::query_as(
        "SELECT classifier_category as name, COUNT(*)::bigint as count
        FROM jeopardy_questions
        WHERE archived = false AND classifier_category IS NOT NULL
        GROUP BY classifier_category
        ORDER BY classifier_category",
    )
    .fetch_all(&state.pool)
    .await?;

    let result: Vec<Value> = rows
        .into_iter()
        .map(|r| json!({ "name": r.name, "count": r.count }))
        .collect();

    Ok(Json(json!(result)))
}
