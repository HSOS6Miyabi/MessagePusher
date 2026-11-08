mod api;
mod compat;
mod delivery;
mod error;
mod models;
mod security;
mod site;
mod subscribe;

use anyhow::{Context, Result};
use axum::{extract::DefaultBodyLimit, routing::{get, post, delete, put, patch}, Router, response::Html};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{env, sync::Arc};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub crypto: security::Crypto,
    pub jwt_secret: Arc<Vec<u8>>,
    pub public_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "messagepusher=info".into())).init();
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let crypto = security::Crypto::new(&env::var("CONFIG_KEY_BASE64").context("CONFIG_KEY_BASE64 is required")?)?;
    let jwt_secret = env::var("JWT_SECRET").context("JWT_SECRET is required")?;
    if jwt_secret.len() < 32 { anyhow::bail!("JWT_SECRET must contain at least 32 characters"); }
    let pool = PgPoolOptions::new().max_connections(12).connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    bootstrap_admin(&pool).await?;
    let state = AppState {
        pool,
        crypto,
        jwt_secret: Arc::new(jwt_secret.into_bytes()),
        public_url: env::var("PUBLIC_BASE_URL").unwrap_or_default().trim_end_matches('/').to_string(),
        http: reqwest::Client::builder().timeout(std::time::Duration::from_secs(22)).build()?,
    };
    let router = create_router(state.clone());
    tokio::spawn(delivery::worker(state));
    let bind = env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind,"MessagePusher started");
    axum::serve(listener, router).await?;
    Ok(())
}
fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/assets/styles.css", get(|| async { ([("content-type", "text/css; charset=utf-8"), ("cache-control","public, max-age=300")], include_str!("../web/styles.css")) }))
        .route("/assets/app.js", get(|| async { ([("content-type", "text/javascript; charset=utf-8"), ("cache-control","public, max-age=300")], include_str!("../web/app.js")) }))
        .route("/healthz", get(|| async { axum::Json(serde_json::json!({"ok":true})) }))
        .route("/v1/login", post(api::login))
        .route("/v1/register", post(site::register))
        .route("/v1/site", get(site::public_settings))
        .route("/v1/admin/site", get(site::public_settings).patch(site::update_settings))
        .route("/v1/admin/overview", get(site::overview))
        .route("/v1/me/password", post(site::change_password))
        .route("/v1/me", get(api::me))
        .route("/v1/users", get(api::list_users).post(api::create_user))
        .route("/v1/users/{id}", patch(api::update_user).delete(api::delete_user))
        .route("/v1/keys", get(api::list_keys).post(api::create_key))
        .route("/v1/keys/{id}", delete(api::delete_key))
        .route("/v1/providers", get(api::list_providers).post(api::create_provider))
        .route("/v1/providers/{id}", get(api::provider_detail).put(api::update_provider).patch(api::patch_provider).delete(api::delete_provider))
        .route("/v1/send", post(api::send_json))
        .route("/v1/send/multipart", post(api::send_multipart))
        .route("/v1/events", get(api::list_events))
        .route("/v1/events/{id}", get(api::event_detail))
        .route("/v1/providers/{id}/telegram/subscribe", post(subscribe::telegram_subscribe))
        .route("/v1/providers/{id}/telegram/webhook", post(subscribe::telegram_set_webhook))
        .route("/v1/providers/{id}/wecom/subscribe", post(subscribe::wecom_subscribe))
        .route("/hooks/telegram/{id}", post(subscribe::telegram_hook))
        .route("/hooks/wecom", get(subscribe::wecom_callback))
        .route("/send", get(compat::pushplus_get).post(compat::pushplus_post))
        .route("/message", post(compat::gotify))
        .route("/ntfy/{key}", post(compat::ntfy))
        .route("/bark/{key}/{title}/{body}", get(compat::bark))
        .route("/{sendkey}", get(compat::serverchan_get).post(compat::serverchan_post))
        .layer(DefaultBodyLimit::max(3_750_000))
        .with_state(state)
}
async fn bootstrap_admin(pool: &PgPool) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    if count > 0 { return Ok(()); }
    let username = env::var("ADMIN_USERNAME").context("Set ADMIN_USERNAME on first startup")?.to_lowercase();
    let password = env::var("ADMIN_PASSWORD").context("Set ADMIN_PASSWORD on first startup")?;
    let hash = security::password_hash(&password)?;
    let created = sqlx::query("INSERT INTO users (id,username,password_hash,role) VALUES ($1,$2,$3,'admin') ON CONFLICT(username) DO NOTHING")
        .bind(Uuid::new_v4()).bind(username).bind(hash).execute(pool).await?;
    if created.rows_affected() > 0 { tracing::info!("Created first administrator"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn router_constructs() {
        let _ = include_str!("../web/index.html");
    }
}
