mod config;
mod db;
mod error;

use axum::{routing::get, Json, Router};
use std::sync::Arc;
use tokio::net::TcpListener;

pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::Config::from_env();
    let pool = db::create_pool(&config.database_url).await;
    let addr = format!("{}:{}", config.host, config.port);

    let state = Arc::new(AppState { pool, config });

    let app = Router::new()
        .route("/api/health", get(health))
        .with_state(state);

    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health(
    state: axum::extract::State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, error::AppError> {
    db::health_check(&state.pool).await?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}
