use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "Authentication required".into())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "Permission denied".into())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "Resource not found".into())
    }
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "Internal request error");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self { Self::internal(error) }
}
impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self { Self::bad(error.to_string()) }
}
pub type ApiResult<T> = Result<T, ApiError>;
