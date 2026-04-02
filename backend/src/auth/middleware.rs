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

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
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
