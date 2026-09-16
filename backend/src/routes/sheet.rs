use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::sheets;
use crate::AppState;

async fn respond(state: &Arc<AppState>, user_id: i32, answer_norm: &str) -> Result<Response, AppError> {
    let sheet = match sheets::ensure_sheet(state, answer_norm).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("sheet generation failed for {answer_norm}: {e:?}");
            None
        }
    };
    match sheet {
        Some(c) => {
            let added = sheets::facts_added(state, user_id, answer_norm).await?;
            let answer = sheets::display_answer(state, answer_norm)
                .await?
                .unwrap_or_else(|| answer_norm.to_string());
            Ok(Json(json!({
                "answerNorm": answer_norm, "answer": answer,
                "identity": c.identity, "facts": c.facts, "factsAdded": added,
            }))
            .into_response())
        }
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn by_answer(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(norm): Path<String>,
) -> Result<Response, AppError> {
    respond(&state, auth.user_id, &norm).await
}

/// Resolve a clue's normalized response server-side (same SQL expression as
/// idx_jq_answer_norm) so practice never has to know the normalization.
pub async fn by_question(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<Response, AppError> {
    let norm: Option<String> = sqlx::query_scalar(
        "SELECT lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))
         FROM jeopardy_questions WHERE id = $1 AND question IS NOT NULL",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;
    match norm {
        Some(n) => respond(&state, auth.user_id, &n).await,
        None => Err(AppError::NotFound("No such question".into())),
    }
}
