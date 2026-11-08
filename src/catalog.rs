use crate::{api, error::{ApiError, ApiResult}, AppState};
use axum::{extract::{Path,State}, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

#[derive(Serialize)]
pub struct ProviderType {
    pub kind: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
}
fn from_row(row:sqlx::postgres::PgRow)->ProviderType {
    ProviderType {
        kind:row.get("kind"),
        label:row.get("label"),
        description:row.get("description"),
        enabled:row.get("enabled"),
    }
}

pub async fn is_enabled(pool:&PgPool,kind:&str)->ApiResult<bool> {
    let result:Option<bool>=sqlx::query_scalar(
        "SELECT enabled FROM provider_catalog WHERE kind=$1"
    ).bind(kind).fetch_optional(pool).await?;
    Ok(result.unwrap_or(false))
}

pub async fn list_public(State(state):State<AppState>)->ApiResult<Json<Vec<ProviderType>>> {
    let rows=sqlx::query(
        "SELECT kind,label,description,enabled FROM provider_catalog WHERE enabled=true ORDER BY sort_order"
    ).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(from_row).collect()))
}

pub async fn list_admin(
    State(state):State<AppState>,headers:HeaderMap
)->ApiResult<Json<Vec<ProviderType>>> {
    let user=api::require_user(&state,&headers).await?;
    if user.role!="admin"{return Err(ApiError::forbidden());}
    let rows=sqlx::query(
        "SELECT kind,label,description,enabled FROM provider_catalog ORDER BY sort_order"
    ).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(from_row).collect()))
}

#[derive(Deserialize)]
pub struct Availability { pub enabled:bool }

pub async fn set_availability(
    State(state):State<AppState>,headers:HeaderMap,
    Path(kind):Path<String>,Json(change):Json<Availability>
)->ApiResult<Json<Value>> {
    let user=api::require_user(&state,&headers).await?;
    if user.role!="admin"{return Err(ApiError::forbidden());}
    let updated=sqlx::query(
        "UPDATE provider_catalog SET enabled=$1,updated_at=now() WHERE kind=$2"
    ).bind(change.enabled).bind(&kind).execute(&state.pool).await?;
    if updated.rows_affected()==0{return Err(ApiError::not_found());}
    Ok(Json(json!({"kind":kind,"enabled":change.enabled})))
}
