use crate::{api, error::{ApiError, ApiResult}, security, AppState};
use axum::{extract::State, http::{HeaderMap, StatusCode}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

#[derive(Serialize, Debug)]
pub struct PublicSite {
    pub site_name: String,
    pub site_description: String,
    pub announcement: String,
    pub allow_registration: bool,
}
impl PublicSite {
    fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        Self {
            site_name: row.get("site_name"),
            site_description: row.get("site_description"),
            announcement: row.get("announcement"),
            allow_registration: row.get("allow_registration"),
        }
    }
}

pub async fn public_settings(State(state): State<AppState>) -> ApiResult<Json<PublicSite>> {
    let row = sqlx::query(
        "SELECT site_name,site_description,announcement,allow_registration FROM site_settings WHERE id=TRUE")
        .fetch_one(&state.pool).await?;
    Ok(Json(PublicSite::from_row(&row)))
}

#[derive(Deserialize)]
pub struct SitePatch {
    pub site_name: Option<String>,
    pub site_description: Option<String>,
    pub announcement: Option<String>,
    pub allow_registration: Option<bool>,
}
fn clean_text(input: String, max_chars: usize, required: bool, field: &str) -> ApiResult<String> {
    let trimmed = input.trim();
    if (required && trimmed.is_empty()) || trimmed.chars().count() > max_chars
        || trimmed.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ApiError::bad(format!("{} must contain at most {} readable characters", field, max_chars)));
    }
    Ok(trimmed.to_owned())
}
pub async fn update_settings(
    State(state): State<AppState>, headers: HeaderMap, Json(input): Json<SitePatch>
) -> ApiResult<Json<PublicSite>> {
    let user = api::require_user(&state, &headers).await?;
    if user.role != "admin" { return Err(ApiError::forbidden()); }
    let name = input.site_name.map(|s| clean_text(s, 60, true, "site_name")).transpose()?;
    let description = input.site_description.map(|s| clean_text(s, 160, false, "site_description")).transpose()?;
    let announcement = input.announcement.map(|s| clean_text(s, 500, false, "announcement")).transpose()?;
    if name.is_none() && description.is_none() && announcement.is_none() && input.allow_registration.is_none() {
        return Err(ApiError::bad("no settings were provided"));
    }
    let row = sqlx::query(
        "UPDATE site_settings SET site_name=COALESCE($1,site_name),
         site_description=COALESCE($2,site_description),
         announcement=COALESCE($3,announcement),
         allow_registration=COALESCE($4,allow_registration),
         updated_at=now() WHERE id=TRUE
         RETURNING site_name,site_description,announcement,allow_registration"
    )
    .bind(name).bind(description).bind(announcement).bind(input.allow_registration)
    .fetch_one(&state.pool).await?;
    Ok(Json(PublicSite::from_row(&row)))
}

#[derive(Deserialize)]
pub struct Registration {
    pub username: String,
    pub password: String,
}
pub async fn register(
    State(state): State<AppState>, Json(input): Json<Registration>
) -> ApiResult<(StatusCode, Json<Value>)> {
    let username = api::valid_username(&input.username)?;
    let registration_enabled: bool = sqlx::query_scalar(
        "SELECT allow_registration FROM site_settings WHERE id=TRUE"
    ).fetch_one(&state.pool).await?;
    if !registration_enabled {return Err(ApiError::forbidden());}
    let hash = security::password_hash(&input.password)
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let mut tx = state.pool.begin().await?;
    // A row lock serializes registration with changes to the registration setting.
    let enabled: bool = sqlx::query_scalar(
        "SELECT allow_registration FROM site_settings WHERE id=TRUE FOR UPDATE"
    ).fetch_one(&mut *tx).await?;
    if !enabled { return Err(ApiError::forbidden()); }
    // A shared hourly budget restricts automated mass registrations.
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE role='user' AND created_at > now() - interval '1 hour'"
    ).fetch_one(&mut *tx).await?;
    if recent >= 60 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "Registration is temporarily busy. Try again later.".into()));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id,username,password_hash,role) VALUES($1,$2,$3,'user')"
    ).bind(id).bind(username).bind(hash).execute(&mut *tx).await.map_err(|err| {
        if err.as_database_error().is_some_and(|db| db.is_unique_violation()) {
            ApiError::bad("username already exists")
        } else {
            ApiError::internal(err)
        }
    })?;
    tx.commit().await?;
    let token = security::session_token(id, 0, &state.jwt_secret).map_err(ApiError::internal)?;
    Ok((StatusCode::CREATED, Json(json!({
        "id": id, "role": "user", "token": token, "expires_in_seconds": 86400
    }))))
}

#[derive(Deserialize)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
}
pub async fn change_password(
    State(state): State<AppState>, headers: HeaderMap, Json(input): Json<PasswordChange>
) -> ApiResult<Json<Value>> {
    let user = api::require_user(&state, &headers).await?;
    let mut tx = state.pool.begin().await?;
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 FOR UPDATE")
        .bind(user.id).fetch_one(&mut *tx).await?;
    if !security::password_valid(&stored, &input.current_password) {
        return Err(ApiError::bad("current password is incorrect"));
    }
    let hash = security::password_hash(&input.new_password)
        .map_err(|e| ApiError::bad(e.to_string()))?;
    if security::password_valid(&stored, &input.new_password) {
        return Err(ApiError::bad("choose a different password"));
    }
    sqlx::query(
        "UPDATE users SET password_hash=$1,session_version=session_version+1 WHERE id=$2"
    ).bind(hash).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"changed":true,"login_required":true})))
}

pub async fn overview(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let user = api::require_user(&state, &headers).await?;
    if user.role != "admin" { return Err(ApiError::forbidden()); }
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE active=true")
        .fetch_one(&state.pool).await?;
    let providers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE enabled=true")
        .fetch_one(&state.pool).await?;
    let today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE created_at > now() - interval '24 hours'"
    ).fetch_one(&state.pool).await?;
    let failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deliveries WHERE status='failed'"
    ).fetch_one(&state.pool).await?;
    Ok(Json(json!({
        "active_users": users,
        "enabled_providers": providers,
        "events_last_24h": today,
        "failed_deliveries": failed
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn site_text_validation() {
        assert!(clean_text(" My Push ".into(), 60, true, "name").is_ok());
        assert!(clean_text("  ".into(), 60, true, "name").is_err());
        assert!(clean_text("a".repeat(61), 60, true, "name").is_err());
        assert!(clean_text("<img onerror=alert(1)>".into(), 60, true, "name").is_ok());
    }
}
