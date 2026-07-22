use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("OPENAI_API_KEY not configured".into()));
    }
    if state
        .pavlov_inflight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(Json(json!({ "started": false, "running": true })));
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::pavlov::run_generation(&st).await {
            tracing::error!("pavlov generation failed (resumable — rerun to continue): {e:?}");
        }
        st.pavlov_inflight.store(false, Ordering::SeqCst);
    });
    Ok(Json(json!({ "started": true })))
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, count(*) FROM pavlov_cues GROUP BY status")
            .fetch_all(&state.pool)
            .await?;
    let get = |k: &str| counts.iter().find(|(s, _)| s == k).map(|(_, n)| *n).unwrap_or(0);
    Ok(Json(json!({
        "running": state.pavlov_inflight.load(Ordering::SeqCst),
        "pending": get("pending"),
        "active": get("active"),
        "dropped": get("dropped"),
    })))
}
