use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::auth::{jwt, password, middleware::AuthUser};
use crate::error::AppError;
use crate::models::user::{LoginRequest, RegisterRequest, User};
use crate::AppState;

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), AppError> {
    // Validate inputs
    if req.username.trim().is_empty() {
        return Err(AppError::BadRequest("Username is required".to_string()));
    }
    if req.email.trim().is_empty() {
        return Err(AppError::BadRequest("Email is required".to_string()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".to_string()));
    }

    // Check email uniqueness
    let existing_email = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&req.email)
    .fetch_optional(&state.pool)
    .await?;

    if existing_email.is_some() {
        return Err(AppError::BadRequest("Email already in use".to_string()));
    }

    // Check username uniqueness
    let existing_username = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = $1"
    )
    .bind(&req.username)
    .fetch_optional(&state.pool)
    .await?;

    if existing_username.is_some() {
        return Err(AppError::BadRequest("Username already in use".to_string()));
    }

    // Hash password
    let password_hash = password::hash_password(&req.password)?;

    // Insert user
    sqlx::query(
        "INSERT INTO users (username, email, password_hash, approved, role) VALUES ($1, $2, $3, false, 'user')"
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
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
    // Find user by email
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&req.email)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    // Check approval status
    if !user.approved {
        return Err(AppError::Unauthorized("Account pending approval".to_string()));
    }

    // Verify password
    let valid = password::verify_password(&req.password, &user.password_hash)?;
    if !valid {
        return Err(AppError::Unauthorized("Invalid email or password".to_string()));
    }

    // Re-hash legacy bcrypt passwords with argon2
    if user.password_hash.starts_with("$2a$") || user.password_hash.starts_with("$2b$") {
        let new_hash = password::hash_password(&req.password)?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(&new_hash)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }

    // Create JWT
    let token = jwt::create_token(user.id, &user.email, &user.role, &state.config.jwt_secret)?;

    // Set cookie
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

pub async fn logout() -> Result<(HeaderMap, Json<Value>), AppError> {
    let cookie = "token=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0";
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((headers, Json(json!({ "message": "Logged out" }))))
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = $1"
    )
    .bind(auth_user.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    Ok(Json(json!({
        "user": {
            "id": user.id,
            "username": user.username,
            "email": user.email,
            "role": user.role,
        }
    })))
}
