mod auth;
mod config;
mod db;
mod error;
mod models;
mod routes;

use axum::{routing::{get, post}, Json, Router};
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
        .route("/api/auth/register", post(routes::auth::register))
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me", get(routes::auth::me))
        .route("/api/quiz/random", get(routes::quiz::random))
        .route("/api/quiz/submit", post(routes::quiz::submit))
        .route("/api/quiz/complete", post(routes::quiz::complete))
        .route("/api/review", get(routes::review::list))
        .route("/api/mastered", get(routes::mastery::random_mastered))
        .route("/api/mastery/reset", post(routes::mastery::reset))
        .route("/api/stats", get(routes::stats::stats))
        .route("/api/categories", get(routes::categories::list))
        .route("/api/preferences", get(routes::preferences::get).put(routes::preferences::update))
        .route("/api/questions/:id", get(routes::questions::get_question))
        .route("/api/questions/:id/archive", post(routes::questions::archive))
        .route("/api/questions/:id/unarchive", post(routes::questions::unarchive))
        .route("/api/coryat", post(routes::coryat::create))
        .route("/api/coryat/history", get(routes::coryat::history))
        .route("/api/coryat/:id", get(routes::coryat::get_game))
        .route("/api/coryat/:id/answer", post(routes::coryat::answer))
        .route("/api/coryat/:id/complete", post(routes::coryat::complete))
        .route("/api/study/generate", post(routes::study::generate))
        .route("/api/study/latest", get(routes::study::latest))
        .route("/api/study/history", get(routes::study::history))
        .route("/api/admin/users", get(routes::admin::list_users))
        .route("/api/admin/approve", post(routes::admin::approve))
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
