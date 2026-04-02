# Jeopardy Training App Secure Rewrite — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the Jeopardy training app from Next.js to Rust (Axum) + Svelte 5, deployed via Cloudflare Tunnel in a distroless container, preserving all existing features.

**Architecture:** Single Axum binary serves a Svelte 5 static SPA and all API routes. PostgreSQL accessed via sqlx with compile-time checked queries. JWT auth in HttpOnly cookies. Cloudflare Tunnel for public access with zero exposed ports.

**Tech Stack:** Rust (Axum, sqlx, tokio, argon2, jsonwebtoken), Svelte 5 (SvelteKit adapter-static, Tailwind CSS, Chart.js), PostgreSQL (existing), Docker (distroless), Cloudflare Tunnel

**Spec:** `docs/superpowers/specs/2026-04-02-secure-rewrite-design.md`

**Existing codebase reference:** The current Next.js app lives in the repo root. The new Rust+Svelte app will be built in a new directory structure alongside it, then replace it.

---

## Phase 1: Rust Backend

### Task 1: Project Scaffold

**Files:**
- Create: `backend/Cargo.toml`
- Create: `backend/src/main.rs`
- Create: `backend/src/config.rs`
- Create: `backend/src/error.rs`
- Create: `backend/src/db.rs`
- Create: `backend/.env` (gitignored)
- Create: `backend/.env.example`
- Modify: `.gitignore`

- [ ] **Step 1: Initialize Cargo project**

```bash
cd /Users/atropos/ai/jeopardy/jeopardy-training-app
mkdir -p backend/src
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "jeopardy-server"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = { version = "0.8", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
jsonwebtoken = "9"
argon2 = "0.5"
bcrypt = "0.16"
tower-http = { version = "0.6", features = ["cors", "compression-gzip", "fs", "set-header", "trace"] }
tower = "0.5"
reqwest = { version = "0.12", features = ["json"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }
rand = "0.9"
```

- [ ] **Step 3: Write config.rs**

Environment config struct loaded from env vars at startup. Fields: `database_url`, `jwt_secret`, `openai_api_key`, `host` (default "0.0.0.0"), `port` (default 3000). Implement `Config::from_env()` that reads each var with `std::env::var()` and panics with a clear message if required vars are missing.

```rust
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub openai_api_key: String,
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set"),
            jwt_secret: std::env::var("JWT_SECRET")
                .expect("JWT_SECRET must be set"),
            openai_api_key: std::env::var("OPENAI_API_KEY")
                .expect("OPENAI_API_KEY must be set"),
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("PORT must be a number"),
        }
    }
}
```

- [ ] **Step 4: Write error.rs**

Unified error type that implements `IntoResponse`. Variants: `BadRequest(String)`, `Unauthorized(String)`, `Forbidden(String)`, `NotFound(String)`, `Internal(String)`. Each maps to the corresponding HTTP status code with a JSON body `{ "error": "message" }`.

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };
        (status, axum::Json(json!({ "error": message }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}
```

- [ ] **Step 5: Write db.rs**

Database pool setup using `sqlx::PgPool`. Function `create_pool(database_url: &str) -> PgPool` that creates a pool with `max_connections(20)` and `min_connections(2)`. Also a `health_check(pool: &PgPool) -> Result<()>` that runs `SELECT 1`.

```rust
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(database_url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .connect(database_url)
        .await
        .expect("Failed to connect to database")
}

pub async fn health_check(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
```

- [ ] **Step 6: Write main.rs**

Minimal main that loads config, creates DB pool, builds an Axum router with a `GET /api/health` endpoint, and starts the server. The health endpoint calls `db::health_check` and returns `{"status": "ok"}`.

```rust
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
```

- [ ] **Step 7: Write .env.example and update .gitignore**

`.env.example`:
```
DATABASE_URL=postgresql://user:pass@localhost/jeopardy
JWT_SECRET=generate-a-random-64-char-string
OPENAI_API_KEY=sk-...
HOST=0.0.0.0
PORT=3000
```

Add to `.gitignore`:
```
backend/target/
backend/.env
```

- [ ] **Step 8: Verify it compiles and connects**

```bash
cd backend
cp .env.example .env
# Edit .env with real DATABASE_URL pointing to tower postgres
cargo build
# This generates Cargo.lock — commit it (needed for Docker build dependency caching)
cargo run
# In another terminal: curl http://localhost:3000/api/health
# Expected: {"status":"ok"}
```

- [ ] **Step 9: Commit**

```bash
git add backend/ .gitignore
git commit -m "feat: scaffold Rust backend with Axum, sqlx, health endpoint"
```

---

### Task 2: Models

**Files:**
- Create: `backend/src/models/mod.rs`
- Create: `backend/src/models/user.rs`
- Create: `backend/src/models/question.rs`
- Create: `backend/src/models/session.rs`
- Create: `backend/src/models/mastery.rs`
- Create: `backend/src/models/coryat.rs`
- Create: `backend/src/models/study.rs`
- Modify: `backend/src/main.rs` (add `mod models`)

These are plain structs with `sqlx::FromRow` and `serde::Serialize` derives. They map directly to the existing PostgreSQL tables. No behavior — just data shapes.

- [ ] **Step 1: Write user.rs**

```rust
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct User {
    pub id: i32,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub created_at: NaiveDateTime,
    pub role: String,
    pub approved: bool,
    pub approved_at: Option<NaiveDateTime>,
    pub game_type_filters: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}
```

- [ ] **Step 2: Write question.rs**

```rust
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct Question {
    pub id: i32,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub category: Option<String>,
    pub classifier_category: Option<String>,
    pub clue_value: Option<i32>,
    pub round: Option<i32>,
    pub air_date: Option<NaiveDate>,
    pub notes: Option<String>,
}
```

- [ ] **Step 3: Write session.rs**

```rust
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct QuizSession {
    pub id: i32,
    pub user_id: i32,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub category_filter: Option<String>,
    pub is_review_session: bool,
}

#[derive(Debug, FromRow, Serialize)]
pub struct QuestionAttempt {
    pub id: i32,
    pub session_id: i32,
    pub question_id: i32,
    pub user_id: i32,
    pub correct: bool,
    pub answered_at: NaiveDateTime,
}
```

- [ ] **Step 4: Write mastery.rs**

```rust
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct QuestionMastery {
    pub id: i32,
    pub user_id: i32,
    pub question_id: i32,
    pub consecutive_correct: i32,
    pub mastered: bool,
    pub mastered_at: Option<NaiveDateTime>,
    pub last_attempt_at: NaiveDateTime,
}
```

- [ ] **Step 5: Write coryat.rs**

```rust
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct CoryatGame {
    pub id: i32,
    pub user_id: i32,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub game_board: serde_json::Value,
    pub jeopardy_score: i32,
    pub double_j_score: i32,
    pub final_score: Option<i32>,
    pub current_round: i32,
    pub questions_answered: i32,
}

#[derive(Debug, Deserialize)]
pub struct CoryatAnswerRequest {
    pub round: String,
    pub col: i32,
    pub row: i32,
    pub response: String,
}
```

- [ ] **Step 6: Write study.rs**

```rust
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct StudyRecommendation {
    pub id: i32,
    pub user_id: i32,
    pub generated_at: NaiveDateTime,
    pub days_analyzed: i32,
    pub analysis: String,
    pub recommendations: serde_json::Value,
    pub question_count: i32,
    pub time_period_start: NaiveDateTime,
    pub time_period_end: NaiveDateTime,
}
```

- [ ] **Step 7: Write mod.rs and wire up**

`backend/src/models/mod.rs`:
```rust
pub mod user;
pub mod question;
pub mod session;
pub mod mastery;
pub mod coryat;
pub mod study;
```

Add `mod models;` to `main.rs`.

- [ ] **Step 8: Verify it compiles**

```bash
cd backend && cargo build
```

- [ ] **Step 9: Commit**

```bash
git add backend/src/models/
git commit -m "feat: add database models for all tables"
```

---

### Task 3: Auth Module

**Files:**
- Create: `backend/src/auth/mod.rs`
- Create: `backend/src/auth/password.rs`
- Create: `backend/src/auth/jwt.rs`
- Create: `backend/src/auth/middleware.rs`
- Modify: `backend/src/main.rs` (add `mod auth`)

- [ ] **Step 1: Write password.rs**

Two functions: `hash_password(password: &str) -> Result<String>` using argon2 with default params, and `verify_password(password: &str, hash: &str) -> Result<bool>` that detects bcrypt (`$2a$`/`$2b$` prefix) vs argon2 (`$argon2` prefix) and uses the appropriate verifier. Returns error for unknown formats.

```rust
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

use crate::error::AppError;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    if hash.starts_with("$2a$") || hash.starts_with("$2b$") {
        // Legacy bcrypt hash
        bcrypt::verify(password, hash)
            .map_err(|e| AppError::Internal(format!("bcrypt verify error: {}", e)))
    } else if hash.starts_with("$argon2") {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| AppError::Internal(format!("Invalid argon2 hash: {}", e)))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    } else {
        Err(AppError::Internal("Unknown password hash format".to_string()))
    }
}
```

- [ ] **Step 2: Write jwt.rs**

Functions: `create_token(user_id: i32, email: &str, role: &str, secret: &str) -> Result<String>` with 30-day expiry, and `validate_token(token: &str, secret: &str) -> Result<Claims>`. Claims struct has `sub` (user_id as string), `email`, `role`, `exp`, `iat`.

```rust
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user_id
    pub email: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_token(
    user_id: i32,
    email: &str,
    role: &str,
    secret: &str,
) -> Result<String, AppError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        role: role.to_string(),
        iat: now.timestamp() as usize,
        exp: (now + chrono::Duration::days(30)).timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("JWT encode error: {}", e)))
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("Invalid or expired token".to_string()))
}
```

- [ ] **Step 3: Write middleware.rs**

An Axum extractor `AuthUser` that reads the `token` cookie, validates the JWT, and provides `user_id: i32`, `email: String`, `role: String` to handlers. Implement `#[axum::async_trait] impl<S> FromRequestParts<S> for AuthUser` where S has `Arc<AppState>` in state.

```rust
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::AppState;

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i32,
    pub email: String,
    pub role: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Arc<AppState>: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = Arc::<AppState>::from_ref(state);

        let cookie_header = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = cookie_header
            .split(';')
            .filter_map(|c| {
                let c = c.trim();
                c.strip_prefix("token=")
            })
            .next()
            .ok_or_else(|| AppError::Unauthorized("No auth token".to_string()))?;

        let claims = super::jwt::validate_token(token, &state.config.jwt_secret)?;
        let user_id: i32 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Unauthorized("Invalid token subject".to_string()))?;

        Ok(AuthUser {
            user_id,
            email: claims.email,
            role: claims.role,
        })
    }
}
```

Note: You'll need `axum::extract::FromRef` — add `use axum::extract::FromRef;` and ensure `AppState` is accessible. The `FromRef` implementation for `Arc<AppState>` is automatic since `Arc<T>` implements `Clone`.

- [ ] **Step 4: Write mod.rs**

```rust
pub mod jwt;
pub mod middleware;
pub mod password;
```

Add `mod auth;` to `main.rs`.

- [ ] **Step 5: Verify it compiles**

```bash
cd backend && cargo build
```

Fix any import issues. The `FromRef` trait may need the `Arc<AppState>` to be used as the router state type directly (i.e., `Router::new().with_state(state)` where `state: Arc<AppState>`).

- [ ] **Step 6: Commit**

```bash
git add backend/src/auth/
git commit -m "feat: add auth module with argon2/bcrypt, JWT, auth middleware"
```

---

### Task 4: Auth Routes

**Files:**
- Create: `backend/src/routes/mod.rs`
- Create: `backend/src/routes/auth.rs`
- Modify: `backend/src/main.rs` (add routes)

- [ ] **Step 1: Write auth.rs**

Four handlers:

**`POST /api/auth/register`**: Accept `RegisterRequest` JSON. Validate email/username not empty, password >= 8 chars. Check email and username uniqueness. Hash password with argon2. Insert user with `approved = false`. Return `201 { "message": "Registration successful. Awaiting admin approval." }`.

**`POST /api/auth/login`**: Accept `LoginRequest` JSON. Find user by email. Check `approved = true`. Verify password (bcrypt or argon2). If bcrypt, re-hash with argon2 and update the row. Create JWT. Set `Set-Cookie: token={jwt}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=2592000`. Return `{ "user": { id, username, email, role } }`.

**`POST /api/auth/logout`**: Clear the token cookie with `Max-Age=0`. Return `{ "message": "Logged out" }`.

**`GET /api/auth/me`**: Requires `AuthUser` extractor. Query user by ID from DB (to get latest role/approved status). Return `{ "user": { id, username, email, role } }`.

```rust
use axum::{extract::State, http::header::SET_COOKIE, Json};
use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::{jwt, password, middleware::AuthUser};
use crate::error::AppError;
use crate::models::user::{LoginRequest, RegisterRequest};
use crate::AppState;

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), AppError> {
    if req.email.is_empty() || req.username.is_empty() {
        return Err(AppError::BadRequest("Email and username are required".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".into()));
    }

    // Check uniqueness
    let existing = sqlx::query_scalar::<_, i32>(
        "SELECT id FROM users WHERE email = $1 OR username = $2 LIMIT 1"
    )
    .bind(&req.email)
    .bind(&req.username)
    .fetch_optional(&state.pool)
    .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest("Email or username already taken".into()));
    }

    let hash = password::hash_password(&req.password)?;

    sqlx::query(
        "INSERT INTO users (username, email, password_hash, approved) VALUES ($1, $2, $3, false)"
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&hash)
    .execute(&state.pool)
    .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "message": "Registration successful. Awaiting admin approval." })),
    ))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<Value>), AppError> {
    let user = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&req.email)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid email or password".into()))?;

    if !user.approved {
        return Err(AppError::Unauthorized("Account pending approval".into()));
    }

    let valid = password::verify_password(&req.password, &user.password_hash)?;
    if !valid {
        return Err(AppError::Unauthorized("Invalid email or password".into()));
    }

    // Migrate bcrypt → argon2 on successful login
    if user.password_hash.starts_with("$2a$") || user.password_hash.starts_with("$2b$") {
        let new_hash = password::hash_password(&req.password)?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(&new_hash)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }

    let token = jwt::create_token(user.id, &user.email, &user.role, &state.config.jwt_secret)?;

    let cookie = format!(
        "token={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=2592000",
        token
    );
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    Ok((
        headers,
        Json(json!({
            "user": {
                "id": user.id,
                "username": user.username,
                "email": user.email,
                "role": user.role,
            }
        })),
    ))
}

pub async fn logout() -> (HeaderMap, Json<Value>) {
    let cookie = "token=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0";
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    (headers, Json(json!({ "message": "Logged out" })))
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users WHERE id = $1"
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found".into()))?;

    Ok(Json(json!({
        "user": {
            "id": user.id,
            "username": user.username,
            "email": user.email,
            "role": user.role,
        }
    })))
}
```

- [ ] **Step 2: Write routes/mod.rs and wire into main.rs**

`routes/mod.rs`:
```rust
pub mod auth;
```

Update `main.rs` to add `mod routes;` and mount the auth routes:
```rust
use axum::routing::{get, post};

let app = Router::new()
    .route("/api/health", get(health))
    .route("/api/auth/register", post(routes::auth::register))
    .route("/api/auth/login", post(routes::auth::login))
    .route("/api/auth/logout", post(routes::auth::logout))
    .route("/api/auth/me", get(routes::auth::me))
    .with_state(state);
```

- [ ] **Step 3: Test auth endpoints manually**

```bash
cd backend && cargo run

# Register
curl -X POST http://localhost:3000/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","email":"test@test.com","password":"testpass123"}'
# Expected: 201 with approval message

# Login (will fail - not approved)
curl -X POST http://localhost:3000/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"test@test.com","password":"testpass123"}'
# Expected: 401 "Account pending approval"

# Login with existing approved user (uses bcrypt hash)
curl -v -X POST http://localhost:3000/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"<existing-user-email>","password":"<password>"}'
# Expected: 200 with Set-Cookie header and user JSON

# Me endpoint with cookie
curl http://localhost:3000/api/auth/me -H "Cookie: token=<jwt-from-login>"
# Expected: 200 with user JSON
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/
git commit -m "feat: add auth routes (register, login, logout, me)"
```

---

### Task 5: Quiz Routes

**Files:**
- Create: `backend/src/routes/quiz.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write quiz.rs**

Three handlers:

**`GET /api/quiz/random`**: Query params: `category` (optional, default "all"), `gameTypes` (optional, comma-separated). Use exponential distribution (lambda=3.5) to bias toward recent questions. Query: questions ordered by `air_date DESC` with filters for non-null fields, non-archived, optional category, optional game-type notes matching. Count total matching questions (can cache in-memory with TTL), generate random offset using exponential distribution, fetch question at that offset.

**`POST /api/quiz/submit`**: Body: `{ questionId, correct, sessionId?, isReviewSession? }`. Create quiz_session if no sessionId provided. Insert question_attempt. Upsert question_mastery: if correct, increment consecutive_correct (set mastered=true if >=3); if incorrect, reset to 0 and mastered=false. Return `{ success, attemptId, sessionId }`.

**`POST /api/quiz/complete`**: Body: `{ sessionId }`. Set completed_at=NOW() on the session. Query all attempts for the session. Return `{ success, summary: { total, correct, accuracy, startedAt, completedAt } }`.

Key implementation detail for exponential distribution:
```rust
let lambda: f64 = 3.5;
let random_value: f64 = rand::random::<f64>();
let exponential_random = -(1.0 - random_value).ln() / lambda;
let normalized_offset = exponential_random.min(1.0);
let offset = (normalized_offset * total_count as f64).floor() as i64;
```

Key implementation detail for game type filtering — build WHERE clause dynamically:
```rust
// gameTypes is comma-separated: "kids,teen,college"
// For each type, add an OR condition:
// "kids" → "notes ILIKE '%Kids%' OR notes ILIKE '%Kid''s%'"
// "teen" → "notes ILIKE '%Teen%'"
// "college" → "notes ILIKE '%College%'"
```

Key implementation detail for mastery upsert:
```sql
INSERT INTO question_mastery (user_id, question_id, consecutive_correct, mastered, mastered_at, last_attempt_at)
VALUES ($1, $2, $3, $4, $5, NOW())
ON CONFLICT (user_id, question_id) DO UPDATE SET
  consecutive_correct = EXCLUDED.consecutive_correct,
  mastered = EXCLUDED.mastered,
  mastered_at = EXCLUDED.mastered_at,
  last_attempt_at = NOW()
```

Include the full handler implementations with all SQL queries, error handling, and response formatting. The `random` handler is the most complex — use `sqlx::query_as` with dynamically built SQL (use `format!` for the WHERE clause, bind params with `sqlx::query_as`). Use `sqlx::query_scalar` for the count.

- [ ] **Step 2: Wire routes into mod.rs and main.rs**

Add to `routes/mod.rs`: `pub mod quiz;`

Add to main.rs router:
```rust
.route("/api/quiz/random", get(routes::quiz::random))
.route("/api/quiz/submit", post(routes::quiz::submit))
.route("/api/quiz/complete", post(routes::quiz::complete))
```

- [ ] **Step 3: Test quiz endpoints manually**

```bash
# Get random question
curl http://localhost:3000/api/quiz/random -H "Cookie: token=<jwt>"
# Expected: Question JSON with id, question, answer, category, etc.

# Submit answer
curl -X POST http://localhost:3000/api/quiz/submit \
  -H "Content-Type: application/json" \
  -H "Cookie: token=<jwt>" \
  -d '{"questionId":1,"correct":true}'
# Expected: { success: true, attemptId: N, sessionId: N }

# Complete session
curl -X POST http://localhost:3000/api/quiz/complete \
  -H "Content-Type: application/json" \
  -H "Cookie: token=<jwt>" \
  -d '{"sessionId":1}'
# Expected: { success: true, summary: {...} }
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/routes/quiz.rs
git commit -m "feat: add quiz routes (random, submit, complete)"
```

---

### Task 6: Review, Mastery, Stats, Categories, Preferences Routes

**Files:**
- Create: `backend/src/routes/review.rs`
- Create: `backend/src/routes/mastery.rs`
- Create: `backend/src/routes/stats.rs`
- Create: `backend/src/routes/categories.rs`
- Create: `backend/src/routes/preferences.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write review.rs**

**`GET /api/review`**: Query param: `category` (optional). Get distinct question_ids where user answered incorrectly. Get mastery records for those questions. Filter out mastered ones. Fetch question details for unmastered, optionally filtered by classifier_category. Return array of `{ question: {...}, masteryProgress: { consecutive_correct, required: 3 } }` sorted by consecutive_correct DESC (closest to mastery first).

SQL approach — use a single query with JOINs:
```sql
SELECT DISTINCT ON (jq.id)
  jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
  jq.clue_value, jq.round, jq.air_date,
  COALESCE(qm.consecutive_correct, 0) as consecutive_correct,
  COALESCE(qm.mastered, false) as mastered
FROM question_attempts qa
JOIN jeopardy_questions jq ON qa.question_id = jq.id
LEFT JOIN question_mastery qm ON qm.question_id = jq.id AND qm.user_id = qa.user_id
WHERE qa.user_id = $1
  AND qa.correct = false
  AND jq.archived = false
  AND COALESCE(qm.mastered, false) = false
  [AND jq.classifier_category = $2]
ORDER BY jq.id, COALESCE(qm.consecutive_correct, 0) DESC
```

- [ ] **Step 2: Write mastery.rs**

**`GET /api/mastered`**: Query param: `category` (optional). Get mastered questions for user. Select random one. Return single question with mastered_at and total_mastered count. Return 404 if none found.

**`POST /api/mastery/reset`**: Body: `{ questionId }`. Delete or update the mastery record: set `consecutive_correct = 0, mastered = false, mastered_at = NULL`. Return `{ success: true }`.

- [ ] **Step 3: Write stats.rs**

**`GET /api/stats`**: Query param: `includeReviewed` (boolean, default false). Four queries:

1. Overall: count total and correct attempts, joining through quiz_sessions if filtering review.
2. Category breakdown: GROUP BY classifier_category, count total and correct.
3. Recent sessions: last 10 sessions with attempt counts.
4. Daily stats: GROUP BY DATE(completed_at), weighted average accuracy.

Use raw SQL for the complex aggregation queries (category breakdown and daily stats). Return the combined stats object.

- [ ] **Step 4: Write categories.rs**

**`GET /api/categories`**: Query distinct classifier_category values with counts from jeopardy_questions where archived=false and classifier_category IS NOT NULL. Return `[{ name, count }]` sorted by name.

```sql
SELECT classifier_category as name, COUNT(*)::int as count
FROM jeopardy_questions
WHERE archived = false AND classifier_category IS NOT NULL
GROUP BY classifier_category
ORDER BY classifier_category
```

- [ ] **Step 5: Write preferences.rs**

**`GET /api/preferences`**: Return user's `game_type_filters` field (JSON string or null).

**`PUT /api/preferences`**: Body: `{ gameTypeFilters: string[] }`. Update user's `game_type_filters` field (store as JSON string). Return `{ success: true }`.

- [ ] **Step 6: Wire all routes and test**

Add all modules to `routes/mod.rs`. Add all routes to main.rs:
```rust
.route("/api/review", get(routes::review::list))
.route("/api/mastered", get(routes::mastery::random_mastered))
.route("/api/mastery/reset", post(routes::mastery::reset))
.route("/api/stats", get(routes::stats::stats))
.route("/api/categories", get(routes::categories::list))
.route("/api/preferences", get(routes::preferences::get).put(routes::preferences::update))
```

Test each endpoint with curl against the real database.

- [ ] **Step 7: Commit**

```bash
git add backend/src/routes/
git commit -m "feat: add review, mastery, stats, categories, preferences routes"
```

---

### Task 7: Questions Routes

**Files:**
- Create: `backend/src/routes/questions.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write questions.rs**

**`GET /api/questions/:id`**: Fetch question by ID. Return full question JSON. Used by Coryat game board to fetch clue details.

**`POST /api/questions/:id/archive`**: Body: `{ reason }`. Set `archived = true, archived_reason = reason, archived_at = NOW()` on the question. Return `{ success: true }`.

**`POST /api/questions/:id/unarchive`**: Set `archived = false, archived_reason = NULL, archived_at = NULL`. Return `{ success: true }`.

- [ ] **Step 2: Wire routes**

```rust
.route("/api/questions/:id", get(routes::questions::get_question))
.route("/api/questions/:id/archive", post(routes::questions::archive))
.route("/api/questions/:id/unarchive", post(routes::questions::unarchive))
```

- [ ] **Step 3: Test and commit**

```bash
curl http://localhost:3000/api/questions/1 -H "Cookie: token=<jwt>"
git add backend/src/routes/questions.rs
git commit -m "feat: add question get/archive/unarchive routes"
```

---

### Task 8: Coryat Routes

**Files:**
- Create: `backend/src/routes/coryat.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

This is the most complex route group. The game board generation has specific business rules.

- [ ] **Step 1: Write coryat.rs — create handler**

**`POST /api/coryat`**: Generate a full game board. Logic:

1. Get top 100 categories by question count (non-archived, non-null category/air_date/classifier_category).
2. Shuffle and pick 6 for Jeopardy round, 6 more for Double Jeopardy.
3. For each category × value pair (5 values per round: J=$200-1000, DJ=$400-2000), find a matching question:
   - Try exact category + clue_value match
   - Fall back to category + round match
   - Fall back to any question in that category
   - Normalize pre-2001 clue values (double them if < 1000 and air_date < 2001-11-26)
4. Assign Daily Doubles: 1 random in Jeopardy, 2 in Double Jeopardy (only on cells with question_id).
5. Find a Final Jeopardy question (round=3, or fallback to any recent question).
6. Build the game_board JSON structure and insert into coryat_games.

Key constants:
```rust
const DOUBLE_VALUE_DATE: &str = "2001-11-26";
const J_VALUES: [i32; 5] = [200, 400, 600, 800, 1000];
const DJ_VALUES: [i32; 5] = [400, 800, 1200, 1600, 2000];
```

- [ ] **Step 2: Write coryat.rs — get, answer, complete, history handlers**

**`GET /api/coryat/:id`**: Fetch game by ID, verify user owns it. Return full game state.

**`POST /api/coryat/:id/answer`**: Body: `CoryatAnswerRequest { round, col, row, response }`. Validate game not completed, question not already answered, question_id not null. Calculate score change based on response ("correct" = +value, "incorrect" = -value, "pass" = 0). Update game_board JSON (set answered field), update score fields, increment questions_answered. Return `{ success, scoreChange, currentRoundScore, totalScore, questionsRemaining, questionsAnswered }`.

**`POST /api/coryat/:id/complete`**: Set completed_at=NOW(), calculate final_score = jeopardy_score + double_j_score. Return `{ success, finalScore }`.

**`GET /api/coryat/history`**: Fetch completed games for user, ordered by completed_at DESC. Return array of game summaries.

- [ ] **Step 3: Wire routes**

```rust
.route("/api/coryat", post(routes::coryat::create))
.route("/api/coryat/history", get(routes::coryat::history))
.route("/api/coryat/:id", get(routes::coryat::get_game))
.route("/api/coryat/:id/answer", post(routes::coryat::answer))
.route("/api/coryat/:id/complete", post(routes::coryat::complete))
```

Note: `/api/coryat/history` must be defined before `/api/coryat/:id` to avoid the path parameter capturing "history".

- [ ] **Step 4: Test game creation and scoring**

```bash
# Create game
curl -X POST http://localhost:3000/api/coryat -H "Cookie: token=<jwt>"
# Expected: gameId and gameBoard JSON with rounds, categories, questions

# Get game
curl http://localhost:3000/api/coryat/<gameId> -H "Cookie: token=<jwt>"

# Answer a question
curl -X POST http://localhost:3000/api/coryat/<gameId>/answer \
  -H "Content-Type: application/json" \
  -H "Cookie: token=<jwt>" \
  -d '{"round":"jeopardy","col":0,"row":0,"response":"correct"}'
# Expected: scoreChange, currentRoundScore, etc.
```

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/coryat.rs
git commit -m "feat: add Coryat game routes with board generation and scoring"
```

---

### Task 9: Study Routes (OpenAI Integration)

**Files:**
- Create: `backend/src/routes/study.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write study.rs — generate handler**

**`POST /api/study/generate`**: Body: `{ days }` (1-365). Logic:

1. Calculate time window: `time_period_end = NOW()`, `time_period_start = NOW() - days`.
2. Query incorrect attempts in that window with question details:
```sql
SELECT qa.*, jq.question, jq.answer, jq.category, jq.classifier_category
FROM question_attempts qa
JOIN jeopardy_questions jq ON qa.question_id = jq.id
WHERE qa.user_id = $1 AND qa.correct = false
  AND qa.answered_at >= $2 AND qa.answered_at <= $3
ORDER BY qa.answered_at DESC
```
3. Group by classifier_category, max 10 per category.
4. Build text: `{i+1}. Clue: "{answer}" Response: "{question}" Original Category: {category}` (note: in Jeopardy, "answer" is the clue and "question" is the response).
5. Call OpenAI Chat Completions API via `reqwest`:
   - URL: `https://api.openai.com/v1/chat/completions`
   - Model: `gpt-4o`
   - Temperature: 0.7
   - Response format: `{ "type": "json_object" }`
   - System message: Jeopardy training analyst prompt (Ken Jennings style, 3-6 topics, concrete strategies, valid sources, Wikipedia links, JSON-only output)
   - User message: Template with wrong answer count, days, and formatted clues
6. Parse JSON response, extract `analysis` and `topics`.
7. Insert into study_recommendations table.
8. Return the recommendation.

Include the full system prompt and user prompt templates as string constants in the file.

- [ ] **Step 2: Write study.rs — history and latest handlers**

**`GET /api/study/latest`**: Fetch most recent recommendation for user. Return it or null.

**`GET /api/study/history`**: Fetch all recommendations for user, ordered by generated_at DESC. Return array.

- [ ] **Step 3: Wire routes**

```rust
.route("/api/study/generate", post(routes::study::generate))
.route("/api/study/latest", get(routes::study::latest))
.route("/api/study/history", get(routes::study::history))
```

- [ ] **Step 4: Test with real OpenAI call**

```bash
curl -X POST http://localhost:3000/api/study/generate \
  -H "Content-Type: application/json" \
  -H "Cookie: token=<jwt>" \
  -d '{"days":30}'
# Expected: AI-generated study recommendation with topics
```

- [ ] **Step 5: Commit**

```bash
git add backend/src/routes/study.rs
git commit -m "feat: add study routes with OpenAI GPT-4o integration"
```

---

### Task 10: Admin Routes

**Files:**
- Create: `backend/src/routes/admin.rs`
- Modify: `backend/src/routes/mod.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write admin.rs**

Both handlers check `auth.role == "admin"`, return 403 if not.

**`GET /api/admin/users`**: Fetch all users (without password_hash). Return array sorted by created_at DESC.

**`POST /api/admin/approve`**: Body: `{ userId }`. Set `approved = true, approved_at = NOW()` for the user. Return `{ success: true }`.

```rust
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    let users = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({ "users": users })))
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    let user_id = req["userId"]
        .as_i64()
        .ok_or_else(|| AppError::BadRequest("userId required".into()))? as i32;

    sqlx::query("UPDATE users SET approved = true, approved_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "success": true })))
}
```

- [ ] **Step 2: Wire routes**

```rust
.route("/api/admin/users", get(routes::admin::list_users))
.route("/api/admin/approve", post(routes::admin::approve))
```

- [ ] **Step 3: Commit**

```bash
git add backend/src/routes/admin.rs
git commit -m "feat: add admin routes (list users, approve)"
```

---

### Task 11: Static File Serving & Security Headers

**Files:**
- Modify: `backend/src/main.rs`
- Modify: `backend/Cargo.toml` (if needed)

- [ ] **Step 1: Add static file serving and security headers to main.rs**

Use `tower_http::services::ServeDir` to serve the Svelte SPA from a `static/` directory. Configure a fallback to `static/index.html` for SPA routing (any path not matching `/api/*` serves the SPA).

Add security headers via `tower_http::set_header::SetResponseHeaderLayer`:
- `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'`
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`

```rust
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use axum::http::HeaderValue;

let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".to_string());

let spa_fallback = ServeFile::new(format!("{}/index.html", static_dir));
let serve_static = ServeDir::new(&static_dir).fallback(spa_fallback);

let api_routes = Router::new()
    .route("/api/health", get(health))
    // ... all /api/* routes ...
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
        HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'"),
    ))
    .layer(tower_http::compression::CompressionLayer::new());
```

- [ ] **Step 2: Add --healthcheck CLI flag**

For Docker health checks, detect `--healthcheck` arg in main. If present, make an HTTP request to `http://localhost:{port}/api/health` and exit 0 or 1.

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a.as_str()) == Some("--healthcheck") {
        // Sync healthcheck — just try TCP connect
        let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
        let addr = format!("127.0.0.1:{}", port);
        match std::net::TcpStream::connect(&addr) {
            Ok(_) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }
    // ... normal async main
}
```

Split main into sync `fn main()` for arg parsing and `async fn run()` for the server.

- [ ] **Step 3: Test with a dummy static directory**

```bash
mkdir -p backend/static
echo '<html><body>Hello</body></html>' > backend/static/index.html
cargo run
curl http://localhost:3000/
# Expected: HTML content
curl -v http://localhost:3000/api/health
# Expected: security headers in response
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/main.rs backend/Cargo.toml
git commit -m "feat: add static file serving, security headers, healthcheck flag"
```

---

## Phase 2: Svelte Frontend

### Task 12: SvelteKit Scaffold

**Files:**
- Create: `frontend/` (SvelteKit project)
- Create: `frontend/svelte.config.js`
- Create: `frontend/vite.config.ts`
- Create: `frontend/tailwind.config.js`
- Create: `frontend/src/app.html`
- Create: `frontend/src/app.css`
- Create: `frontend/static/` (favicon, manifest)

- [ ] **Step 1: Create SvelteKit project**

```bash
cd /Users/atropos/ai/jeopardy/jeopardy-training-app
npx sv create frontend --template minimal --types ts
cd frontend
npm install
```

- [ ] **Step 2: Install dependencies**

```bash
npm install -D @sveltejs/adapter-static tailwindcss @tailwindcss/vite
npm install chart.js svelte-chartjs
```

- [ ] **Step 3: Configure adapter-static**

`frontend/svelte.config.js`:
```javascript
import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: 'index.html', // SPA mode
    }),
  },
};
```

- [ ] **Step 4: Configure Tailwind**

`frontend/vite.config.ts`:
```typescript
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    proxy: {
      '/api': 'http://localhost:3000',
    },
  },
});
```

`frontend/tailwind.config.js`:
```javascript
export default {
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    extend: {
      colors: {
        jeopardy: {
          blue: '#060CE9',
          gold: '#FFD700',
        },
      },
    },
  },
};
```

`frontend/src/app.css`:
```css
@import 'tailwindcss';
```

- [ ] **Step 5: Set up SPA routing layout**

`frontend/src/routes/+layout.ts`:
```typescript
export const ssr = false;
export const prerender = false;
```

`frontend/src/routes/+layout.svelte`:
```svelte
<script>
  import '../app.css';
  let { children } = $props();
</script>

{@render children()}
```

- [ ] **Step 6: Create PWA manifest and icons**

`frontend/static/manifest.json`:
```json
{
  "name": "Jeopardy Training",
  "short_name": "Jeopardy",
  "start_url": "/dashboard",
  "display": "standalone",
  "background_color": "#060CE9",
  "theme_color": "#060CE9",
  "icons": [
    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" }
  ]
}
```

Add to `frontend/src/app.html` `<head>`:
```html
<link rel="manifest" href="/manifest.json" />
<meta name="theme-color" content="#060CE9" />
<meta name="apple-mobile-web-app-capable" content="yes" />
<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
```

Create placeholder icon PNGs (192x192 and 512x512) — blue square with gold "J".

- [ ] **Step 7: Create placeholder landing page**

`frontend/src/routes/+page.svelte`:
```svelte
<div class="min-h-screen bg-jeopardy-blue flex items-center justify-center">
  <h1 class="text-jeopardy-gold text-4xl font-bold">Jeopardy Training</h1>
</div>
```

- [ ] **Step 8: Verify dev server works**

```bash
cd frontend && npm run dev
# Open http://localhost:5173 — should show gold text on blue background
# API calls should proxy to backend on port 3000
```

- [ ] **Step 9: Commit**

```bash
git add frontend/
git commit -m "feat: scaffold SvelteKit project with adapter-static and Tailwind"
```

---

### Task 13: API Layer & Auth Store

**Files:**
- Create: `frontend/src/lib/api.ts`
- Create: `frontend/src/lib/auth.svelte.ts`

- [ ] **Step 1: Write api.ts**

Thin fetch wrapper. All requests include `credentials: 'same-origin'` (cookies auto-sent). Handles JSON parsing and error responses. Methods: `get(path, params?)`, `post(path, body?)`, `put(path, body?)`. On 401, redirect to `/login`.

```typescript
class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function request(method: string, path: string, body?: unknown): Promise<unknown> {
  const opts: RequestInit = {
    method,
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
  };
  if (body) opts.body = JSON.stringify(body);

  const res = await fetch(path, opts);

  if (res.status === 401) {
    window.location.href = '/login';
    throw new ApiError(401, 'Unauthorized');
  }

  if (!res.ok) {
    const data = await res.json().catch(() => ({ error: 'Request failed' }));
    throw new ApiError(res.status, data.error || 'Request failed');
  }

  return res.json();
}

export const api = {
  get: (path: string) => request('GET', path),
  post: (path: string, body?: unknown) => request('POST', path, body),
  put: (path: string, body?: unknown) => request('PUT', path, body),
};
```

- [ ] **Step 2: Write auth.svelte.ts**

Svelte 5 runes-based auth store. Exports reactive state: `user` (object or null), `loading` (boolean). Functions: `checkAuth()` — calls `GET /api/auth/me`, sets user or null. `login(email, password)` — calls `POST /api/auth/login`, sets user. `logout()` — calls `POST /api/auth/logout`, clears user, redirects to `/login`. `register(username, email, password)` — calls `POST /api/auth/register`.

```typescript
import { api } from './api';

interface User {
  id: number;
  username: string;
  email: string;
  role: string;
}

let user = $state<User | null>(null);
let loading = $state(true);

export function getAuth() {
  return {
    get user() { return user; },
    get loading() { return loading; },
  };
}

export async function checkAuth() {
  try {
    const data = await api.get('/api/auth/me') as { user: User };
    user = data.user;
  } catch {
    user = null;
  } finally {
    loading = false;
  }
}

export async function login(email: string, password: string) {
  const data = await api.post('/api/auth/login', { email, password }) as { user: User };
  user = data.user;
}

export async function logout() {
  await api.post('/api/auth/logout');
  user = null;
  window.location.href = '/login';
}

export async function register(username: string, email: string, password: string) {
  await api.post('/api/auth/register', { username, email, password });
}
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/
git commit -m "feat: add API layer and auth store"
```

---

### Task 14: Layout & Navigation

**Files:**
- Create: `frontend/src/lib/components/Nav.svelte`
- Modify: `frontend/src/routes/+layout.svelte`

- [ ] **Step 1: Write Nav.svelte**

Port the existing Navigation component. Desktop: horizontal nav bar with links. Mobile: hamburger menu. Props: none (reads from auth store). Links: Quiz, Coryat, Review, Mastered, Study, Dashboard, Settings. Admin link if role=admin. Logout button. Styling: `bg-jeopardy-blue` bar, `text-jeopardy-gold` links, hamburger icon on mobile.

Match the existing app's navigation structure exactly. Use Svelte 5 `$state` for mobile menu toggle.

- [ ] **Step 2: Update +layout.svelte**

Add auth check on mount. Show loading spinner while checking. Show Nav on all pages except login/register/landing. Use `$effect` to call `checkAuth()` on mount.

```svelte
<script>
  import '../app.css';
  import Nav from '$lib/components/Nav.svelte';
  import { checkAuth, getAuth } from '$lib/auth.svelte';
  import { page } from '$app/stores';

  let { children } = $props();
  const auth = getAuth();

  const publicPaths = ['/', '/login', '/register'];

  $effect(() => {
    checkAuth();
  });
</script>

{#if auth.loading}
  <div class="min-h-screen flex items-center justify-center">
    <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
  </div>
{:else}
  {#if auth.user && !publicPaths.includes($page.url.pathname)}
    <Nav />
  {/if}
  {@render children()}
{/if}
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/
git commit -m "feat: add navigation component and auth-aware layout"
```

---

### Task 15: Auth Pages (Login, Register, Landing)

**Files:**
- Modify: `frontend/src/routes/+page.svelte` (landing)
- Create: `frontend/src/routes/login/+page.svelte`
- Create: `frontend/src/routes/register/+page.svelte`

- [ ] **Step 1: Write landing page**

Port the existing landing page. Blue background, gold text, "Jeopardy! Training" title, tagline about 500K+ questions, Login and Register buttons linking to respective pages.

- [ ] **Step 2: Write login page**

Form with email and password fields, password visibility toggle, submit button. On submit, call `login()` from auth store. On success, redirect to `/dashboard`. Show error messages on failure.

- [ ] **Step 3: Write register page**

Form with username, email, password fields. On submit, call `register()`. On success, show "Registration successful. Awaiting admin approval." message with link to login.

- [ ] **Step 4: Test full auth flow in browser**

Start backend and frontend dev servers. Register → see approval message → (manually approve in DB or via admin endpoint) → Login → see dashboard redirect.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/
git commit -m "feat: add landing, login, and register pages"
```

---

### Task 16: Dashboard Page

**Files:**
- Create: `frontend/src/routes/dashboard/+page.svelte`
- Create: `frontend/src/lib/components/StatsChart.svelte`

- [ ] **Step 1: Write dashboard page**

Port from existing `app/dashboard/page.tsx`. On mount, fetch `GET /api/stats?includeReviewed={toggle}`. Display:
- Overall stats cards (total, correct, accuracy %)
- Daily performance line chart (Chart.js Line via svelte-chartjs)
- Category accuracy bar chart (Chart.js Bar with dynamic colors: green >=75%, amber 50-74%, red <50%)
- Category detail table sorted by accuracy ASC
- Toggle checkbox for include/exclude review sessions
- Action buttons: Start Quiz, Review Wrong Answers, Mastered Questions, Start Coryat Game

Chart config: X-axis dates angled -45 degrees, Y-axis 0-100, responsive, tooltips enabled.

- [ ] **Step 2: Write StatsChart.svelte**

Reusable chart component wrapping Chart.js. Props: `type` ("line" | "bar"), `data`, `options`. Use `svelte-chartjs` `Line` and `Bar` components.

- [ ] **Step 3: Test dashboard renders with real data**

Navigate to `/dashboard` after login. Verify stats, charts, and action buttons all render correctly.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/
git commit -m "feat: add dashboard page with stats and charts"
```

---

### Task 17: Quiz Page

**Files:**
- Create: `frontend/src/routes/quiz/+page.svelte`
- Create: `frontend/src/lib/components/QuestionCard.svelte`
- Create: `frontend/src/lib/components/CategoryFilter.svelte`
- Create: `frontend/src/lib/components/SessionSummary.svelte`

- [ ] **Step 1: Write QuestionCard.svelte**

Port from existing `components/QuestionCard.tsx`. Props: `clue`, `answer`, `category`, `classifierCategory`, `clueValue`, `round`, `airDate`, `showAnswer`, `onRevealAnswer`, `onCorrect`, `onIncorrect`, `badge?`, `additionalActions?`, `archiveButton?`, `keyboardHint?`, `cardBgColor?` (default "bg-jeopardy-blue"), `cardTextColor?` (default "text-jeopardy-gold"), `buttonBgColor?`, `buttonTextColor?`, `submitting?`.

Shows clue text on colored card. "Show Answer" button. After reveal: answer in white box, Correct/Incorrect buttons. Footer with category, classifier category, value, air date. Responsive sizing.

- [ ] **Step 2: Write CategoryFilter.svelte**

Dropdown select with "All Categories" default plus category options with counts. Props: `categories: { name, count }[]`, `selected: string`, `onchange`.

- [ ] **Step 3: Write SessionSummary.svelte**

Modal overlay showing session results. Props: `summary: { total, correct, accuracy, startedAt, completedAt }`, `onclose`. Displays accuracy %, correct/total count, time range.

- [ ] **Step 4: Write quiz page**

Port from existing `app/quiz/page.tsx`. Full state machine:

1. On mount: fetch categories and user preferences (game type filters)
2. Fetch first question
3. Main loop: show question → keyboard/button interaction → reveal → mark → prefetch next → submit → show next
4. Keyboard: Space=reveal, ArrowRight=correct, ArrowLeft=incorrect (ignore if focused on input/select)
5. Prefetch: when answer revealed, background-fetch next question. On submit, use prefetched if available.
6. Running stats in header: "X/Y correct (Z%)"
7. End session: confirmation → POST complete → show SessionSummary modal
8. Game type filters: checkboxes for kids/teen/college, persist via PUT /api/preferences
9. Archive button on revealed questions

All API calls: `GET /api/categories`, `GET /api/preferences`, `PUT /api/preferences`, `GET /api/quiz/random?category=X&gameTypes=Y`, `POST /api/quiz/submit`, `POST /api/quiz/complete`, `POST /api/questions/:id/archive`.

- [ ] **Step 5: Test full quiz flow**

Start quiz → answer questions → verify prefetching works → end session → verify summary modal.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/
git commit -m "feat: add quiz page with prefetching, keyboard shortcuts, session tracking"
```

---

### Task 18: Review Page

**Files:**
- Create: `frontend/src/routes/review/+page.svelte`
- Create: `frontend/src/lib/components/MasteryBadge.svelte`

- [ ] **Step 1: Write MasteryBadge.svelte**

Shows mastery progress as a colored badge. Props: `consecutiveCorrect: number`, `required: number` (default 3). Colors: 0=red, 1=yellow, 2=orange. Text: "Progress: X/Y checkmark".

- [ ] **Step 2: Write review page**

Port from existing `app/review/page.tsx`. Two modes:

**List view**: Expandable cards showing wrong answers. Each card has: classifier category badge, clue value, mastery progress badge, truncated clue text. Click to expand: shows correct response, original category, air date, archive button.

**Review session mode**: "Start Review Session" button. Presents questions one at a time using QuestionCard. Progress bar showing X of Y. Submits via `POST /api/quiz/submit` with `isReviewSession: true`. End session button.

Category filter dropdown at top.

API calls: `GET /api/categories`, `GET /api/review?category=X`, `POST /api/quiz/submit`, `POST /api/questions/:id/archive`.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/
git commit -m "feat: add review page with list view and review session mode"
```

---

### Task 19: Mastered Page

**Files:**
- Create: `frontend/src/routes/mastered/+page.svelte`

- [ ] **Step 1: Write mastered page**

Port from existing `app/mastered/page.tsx`. Single-question carousel view with green theme. On mount, fetch random mastered question. Shows: mastered count, QuestionCard with green styling (`bg-green-700`, `text-yellow-300`), mastery date badge, "Reset Mastery" button (with confirmation), "Next Question" button. Keyboard: Space=reveal. Category filter.

API calls: `GET /api/categories`, `GET /api/mastered?category=X`, `POST /api/quiz/submit` (with isReviewSession true), `POST /api/mastery/reset`.

- [ ] **Step 2: Commit**

```bash
git add frontend/src/routes/mastered/
git commit -m "feat: add mastered questions page with green theme and reset"
```

---

### Task 20: Coryat Pages

**Files:**
- Create: `frontend/src/routes/coryat/+page.svelte` (lobby)
- Create: `frontend/src/routes/coryat/[gameId]/+page.svelte` (game board)
- Create: `frontend/src/routes/coryat/history/+page.svelte`
- Create: `frontend/src/lib/components/GameBoard.svelte`

- [ ] **Step 1: Write GameBoard.svelte**

6x5 grid component for a single round. Props: `categories: string[]`, `questions: CellData[]`, `onSelect: (col, row) => void`. Renders category headers row, then 5 value rows. Cells show dollar value or "—" if no question. Answered cells are grayed out. Daily double cells show red "DD" badge. Responsive grid using CSS grid.

- [ ] **Step 2: Write Coryat lobby page**

Port from existing `app/coryat/page.tsx`. Shows: explanation of Coryat scoring, benchmark reference table, user stats (total games, average score, best score), resume incomplete game link, "Start New Game" button, "View History" link.

API calls: `POST /api/coryat` (create), `GET /api/coryat/history` (for stats).

- [ ] **Step 3: Write Coryat game board page**

Port from existing `app/coryat/[gameId]/page.tsx`. Most complex frontend page. On mount, fetch game state. Display:
- Current round header + score
- GameBoard grid for current round
- Cell click → fetch question details via `GET /api/questions/:id` → show clue modal
- "Reveal Answer" button in modal
- Three response buttons: Correct (green), Incorrect (red), Pass (gray)
- On answer: `POST /api/coryat/:id/answer` → update board state + score
- Round complete modal when all questions in round answered
- "Continue to Double Jeopardy" / "Continue to Final Jeopardy" buttons
- Game complete modal with score breakdown
- Final Jeopardy: single clue (not scored)

- [ ] **Step 4: Write Coryat history page**

Table of completed games: date, Jeopardy score, Double Jeopardy score, total, sorted by date DESC.

- [ ] **Step 5: Test full Coryat flow**

Create game → play through Jeopardy round → continue to DJ → Final → complete → verify history.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/
git commit -m "feat: add Coryat pages (lobby, game board, history)"
```

---

### Task 21: Study Page

**Files:**
- Create: `frontend/src/routes/study/+page.svelte`

- [ ] **Step 1: Write study page**

Port from existing `app/study/page.tsx`. Sections:

**Generate form**: "Analyze last X days" number input (1-365), Generate button with loading spinner ("Analyzing with AI...").

**Latest recommendation display**: Analysis summary in blue box, then topic cards with: topic heading, explanation, readings list (book emoji), Wikipedia links (link emoji, cleaned URL text), strategies list (lightbulb emoji).

**History section**: Expandable list of past recommendations. Each shows: date, days analyzed, question count, topic count. Expand to see analysis text and compact topic summaries.

API calls: `GET /api/study/latest`, `GET /api/study/history`, `POST /api/study/generate`.

- [ ] **Step 2: Commit**

```bash
git add frontend/src/routes/study/
git commit -m "feat: add study recommendations page with AI generation and history"
```

---

### Task 22: Settings & Admin Pages

**Files:**
- Create: `frontend/src/routes/settings/+page.svelte`
- Create: `frontend/src/routes/admin/+page.svelte`

- [ ] **Step 1: Write settings page**

Display user profile info (username, email, role). Placeholder for future settings expansion.

- [ ] **Step 2: Write admin page**

Admin-only page (redirect non-admins). Fetch `GET /api/admin/users`. Display table of users: username, email, role, approved status, created date. "Approve" button for unapproved users → `POST /api/admin/approve`.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/
git commit -m "feat: add settings and admin pages"
```

---

## Phase 3: Build & Deploy

### Task 23: Multi-Stage Dockerfile

**Files:**
- Create: `Dockerfile` (repo root, replaces existing)
- Modify: `.dockerignore`

- [ ] **Step 1: Write Dockerfile**

Three-stage build:

```dockerfile
# Stage 1: Build frontend
FROM node:22-alpine AS frontend-build
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:bookworm AS rust-build
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
# Create dummy src for dependency caching
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src target/release/jeopardy-server

COPY backend/src ./src
COPY --from=frontend-build /app/frontend/build ./static
RUN cargo build --release

# Stage 3: Runtime
FROM gcr.io/distroless/cc-debian12
COPY --from=rust-build /app/target/release/jeopardy-server /app/server
COPY --from=rust-build /app/static /app/static
ENV STATIC_DIR=/app/static
EXPOSE 3000
ENTRYPOINT ["/app/server"]
```

Note: sqlx compile-time checking requires a DATABASE_URL at build time. Options:
- Use `sqlx prepare` to generate offline query data, then build with `SQLX_OFFLINE=true`
- OR set DATABASE_URL as a build arg pointing to the real DB

Recommended: use `sqlx prepare` workflow. Add a step before the Docker build:
```bash
cd backend
cargo sqlx prepare
# This generates .sqlx/ directory with query metadata
```

Then the Dockerfile uses `SQLX_OFFLINE=true`:
```dockerfile
COPY backend/.sqlx ./.sqlx
ENV SQLX_OFFLINE=true
RUN cargo build --release
```

- [ ] **Step 2: Write .dockerignore**

```
backend/target/
frontend/node_modules/
frontend/.svelte-kit/
.git/
*.md
docs/
```

- [ ] **Step 3: Test Docker build locally**

```bash
cd backend && cargo sqlx prepare
cd ..
docker build -t jeopardy-training-app:test .
docker run --rm -p 3000:3000 \
  -e DATABASE_URL="postgresql://..." \
  -e JWT_SECRET="test-secret" \
  -e OPENAI_API_KEY="sk-..." \
  jeopardy-training-app:test
# Verify: curl http://localhost:3000/ returns SPA
# Verify: curl http://localhost:3000/api/health returns ok
```

- [ ] **Step 4: Commit**

```bash
git add Dockerfile .dockerignore
git commit -m "feat: add multi-stage Dockerfile (frontend + Rust + distroless)"
```

---

### Task 24: Docker Compose & Cloudflare Tunnel

**Files:**
- Create: `docker-compose.yml` (replaces existing)
- Create: `docker-compose.prod.yml` (production overrides)

- [ ] **Step 1: Write docker-compose.yml**

```yaml
services:
  jeopardy:
    image: ghcr.io/ebertx/jeopardy-training-app:latest
    restart: unless-stopped
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    env_file: .env
    networks:
      - jeopardy-internal
    healthcheck:
      test: ["/app/server", "--healthcheck"]
      interval: 30s
      timeout: 5s
      retries: 3

  cloudflared:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel run
    environment:
      - TUNNEL_TOKEN=${TUNNEL_TOKEN}
    networks:
      - jeopardy-internal
    depends_on:
      jeopardy:
        condition: service_healthy

networks:
  jeopardy-internal:
    driver: bridge
```

- [ ] **Step 2: Write GitHub Actions workflow for CI/CD**

Create `.github/workflows/build.yml`:
- On push to main
- Build Docker image using multi-stage Dockerfile
- Push to ghcr.io/ebertx/jeopardy-training-app:latest
- Watchtower on Tower will auto-pull

```yaml
name: Build and Push

on:
  push:
    branches: [main]

jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Set up sqlx offline data
        run: |
          cd backend
          # .sqlx directory should be committed
          test -d .sqlx || echo "WARNING: .sqlx offline data missing"

      - name: Build and push
        uses: docker/build-push-action@v5
        with:
          context: .
          push: true
          tags: ghcr.io/ebertx/jeopardy-training-app:latest
```

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml .github/
git commit -m "feat: add Docker Compose with Cloudflare Tunnel and CI/CD workflow"
```

---

### Task 25: Deployment & Migration Cutover

**Files:** Server-side changes on Tower (via tower-ssh)

This task is executed manually, not by subagents.

- [ ] **Step 1: Rotate secrets**

Generate new JWT_SECRET, rotate PostgreSQL password, rotate OpenAI API key. Update `.env` on Tower at the deployment location.

- [ ] **Step 2: Set up Cloudflare Tunnel**

Create tunnel in Cloudflare dashboard. Configure `jeopardy.ebertx.com` → `http://jeopardy:3000`. Get tunnel token.

- [ ] **Step 3: Bind PostgreSQL to localhost/Docker**

Change the PostgreSQL container to NOT publish port 5432 to `0.0.0.0`. Instead, only expose on the Docker internal network or `127.0.0.1`.

- [ ] **Step 4: Deploy new stack**

```bash
tower-ssh "cd /path/to/deploy && docker-compose pull && docker-compose up -d"
```

- [ ] **Step 5: Verify all features**

Test: login, quiz, review, mastered, coryat, study, admin, dashboard.

- [ ] **Step 6: Remove old Traefik routing**

Remove jeopardy-related Traefik labels/config. The app no longer routes through Traefik.

- [ ] **Step 7: Clean up database**

```sql
DROP TABLE IF EXISTS auth_sessions;
```

- [ ] **Step 8: Update security monitor**

Add new container name to KNOWN_CONTAINERS in `/boot/config/custom/container-security-monitor.sh`. Add scan_container call with expected process pattern "jeopardy-server" (just the Rust binary).

- [ ] **Step 9: Scrub secrets from git history**

```bash
pip install git-filter-repo
git filter-repo --path .env --invert-paths
```

Force push (with confirmation) to rewrite history.

- [ ] **Step 10: Update Tower changelog**

Log all deployment changes to `/boot/config/custom/changelog.md`.
