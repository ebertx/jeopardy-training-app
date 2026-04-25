mod auth;
mod config;
mod db;
mod error;
mod models;
mod routes;

use axum::{routing::{get, post}, Json, Router};
use axum::http::HeaderValue;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a.as_str()) == Some("--healthcheck") {
        let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
        let addr = format!("127.0.0.1:{}", port);
        match std::net::TcpStream::connect(&addr) {
            Ok(_) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}

async fn run() {
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

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".to_string());
    let spa_fallback = ServeFile::new(format!("{}/index.html", static_dir));
    let serve_static = ServeDir::new(&static_dir).fallback(spa_fallback);

    let api_routes = Router::new()
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
        .route("/api/questions/{id}", get(routes::questions::get_question))
        .route("/api/questions/{id}/archive", post(routes::questions::archive))
        .route("/api/questions/{id}/unarchive", post(routes::questions::unarchive))
        .route("/api/coryat", post(routes::coryat::create))
        .route("/api/coryat/history", get(routes::coryat::history))
        .route("/api/coryat/{id}", get(routes::coryat::get_game))
        .route("/api/coryat/{id}/answer", post(routes::coryat::answer))
        .route("/api/coryat/{id}/complete", post(routes::coryat::complete))
        .route("/api/study/generate", post(routes::study::generate))
        .route("/api/study/latest", get(routes::study::latest))
        .route("/api/study/history", get(routes::study::history))
        .route("/api/admin/users", get(routes::admin::list_users))
        .route("/api/admin/approve", post(routes::admin::approve))
        .with_state(state);

    let app = Router::new()
        .merge(api_routes)
        .fallback_service(serve_static)
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'"),
        ))
        .layer(tower_http::compression::CompressionLayer::new());

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
