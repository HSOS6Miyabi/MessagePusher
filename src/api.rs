use crate::{error::{ApiError, ApiResult}, models::{Attachment, DeliveryMode, ProviderInput, ProviderSecrets, SendRequest}, security, AppState};
use axum::{extract::{Multipart, Path, State}, http::{HeaderMap, StatusCode}, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, PgPool};
use uuid::Uuid;

#[derive(Clone, serde::Serialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub active: bool,
}

fn bearer(headers: &HeaderMap) -> ApiResult<&str> {
    headers.get("authorization").and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| !s.is_empty())
        .ok_or_else(ApiError::unauthorized)
}

pub async fn require_user(state: &AppState, headers: &HeaderMap) -> ApiResult<User> {
    let (id, version) = security::verify_session(bearer(headers)?, &state.jwt_secret)
        .map_err(|_| ApiError::unauthorized())?;
    let row = sqlx::query("SELECT id,username,role,active,session_version FROM users WHERE id=$1")
        .bind(id).fetch_optional(&state.pool).await?.ok_or_else(ApiError::unauthorized)?;
    if !row.get::<bool,_>("active") { return Err(ApiError::forbidden()); }
    if row.get::<i64,_>("session_version") != version { return Err(ApiError::unauthorized()); }
    Ok(User {id:row.get("id"),username:row.get("username"),role:row.get("role"),active:true})
}
fn admin_only(user: &User) -> ApiResult<()> {
    if user.role != "admin" { return Err(ApiError::forbidden()); }
    Ok(())
}
pub async fn user_from_key(state: &AppState, key: &str) -> ApiResult<Uuid> {
    if key.len() < 24 || key.len() > 128 { return Err(ApiError::unauthorized()); }
    let hash = security::hash_secret(key);
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM api_keys k JOIN users u ON u.id=k.user_id WHERE k.key_hash=$1 AND k.revoked_at IS NULL AND u.active=true")
        .bind(hash).fetch_optional(&state.pool).await?;
    id.ok_or_else(ApiError::unauthorized)
}
#[derive(Deserialize)]
pub struct LoginBody { username: String, password: String }
pub async fn login(State(state): State<AppState>, Json(body): Json<LoginBody>) -> ApiResult<Json<Value>> {
    let row = sqlx::query("SELECT id,password_hash,active,session_version FROM users WHERE username=$1")
        .bind(body.username.to_lowercase()).fetch_optional(&state.pool).await?;
    let Some(row) = row else { return Err(ApiError::unauthorized()); };
    if !row.get::<bool,_>("active") || !security::password_valid(row.get("password_hash"), &body.password) {
        return Err(ApiError::unauthorized());
    }
    let user_id: Uuid = row.get("id");
    let token = security::session_token(user_id, row.get::<i64,_>("session_version"), &state.jwt_secret).map_err(ApiError::internal)?;
    Ok(Json(json!({"token":token,"expires_in_seconds":86400})))
}
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<User>> {
    Ok(Json(require_user(&state,&headers).await?))
}
pub async fn list_users(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Vec<Value>>> {
    admin_only(&require_user(&state,&headers).await?)?;
    let rows = sqlx::query("SELECT id,username,role,active,created_at FROM users ORDER BY created_at DESC")
        .fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(|r| json!({
        "id":r.get::<Uuid,_>("id"),"username":r.get::<String,_>("username"),
        "role":r.get::<String,_>("role"),"active":r.get::<bool,_>("active"),
        "created_at":r.get::<DateTime<Utc>,_>("created_at")
    })).collect()))
}
pub fn valid_username(value: &str) -> ApiResult<String> {
    let username = value.trim().to_lowercase();
    if username.len()<3 || username.len()>60 || !username.chars().all(|c| c.is_ascii_alphanumeric() || c=='_' || c=='-') {
        return Err(ApiError::bad("username must be 3 to 60 letters, digits, underscores or hyphens"));
    }
    Ok(username)
}
#[derive(Deserialize)]
pub struct NewUser { username:String, password:String, #[serde(default="default_user")] role:String }
fn default_user() -> String {"user".into()}
pub async fn create_user(State(state):State<AppState>,headers:HeaderMap,Json(req):Json<NewUser>) -> ApiResult<(StatusCode,Json<Value>)> {
    admin_only(&require_user(&state,&headers).await?)?;
    let username = valid_username(&req.username)?;
    if !["admin","user"].contains(&req.role.as_str()) {return Err(ApiError::bad("role must be admin or user"));}
    let hash=security::password_hash(&req.password).map_err(|e|ApiError::bad(e.to_string()))?;
    let id=Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,$4)")
        .bind(id).bind(username).bind(hash).bind(&req.role).execute(&state.pool).await
        .map_err(|e|if e.as_database_error().is_some_and(|db|db.is_unique_violation()){ApiError::bad("username already exists")}else{ApiError::internal(e)})?;
    Ok((StatusCode::CREATED,Json(json!({"id":id,"role":req.role}))))
}
#[derive(Deserialize)]
pub struct UserPatch {role:Option<String>, active:Option<bool>,password:Option<String>}
pub async fn update_user(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>,Json(patch):Json<UserPatch>) -> ApiResult<Json<Value>> {
    let caller=require_user(&state,&headers).await?;
    admin_only(&caller)?;
    if let Some(role)=&patch.role {if role!="admin"&&role!="user"{return Err(ApiError::bad("invalid role"));}}
    if caller.id==id && (patch.active==Some(false) || patch.role.as_deref()==Some("user")){
        return Err(ApiError::bad("you cannot deactivate or demote your own administrator account"));
    }
    let hash=patch.password.as_deref().map(security::password_hash).transpose()
        .map_err(|e|ApiError::bad(e.to_string()))?;
    let mut tx=state.pool.begin().await?;
    sqlx::query("SELECT id FROM site_settings WHERE id=TRUE FOR UPDATE")
        .fetch_one(&mut *tx).await?;
    if patch.role.as_deref()==Some("user")||patch.active==Some(false){
        protect_last_admin(&mut tx,id).await?;
    }
    let changed=sqlx::query("UPDATE users SET role=COALESCE($1,role),active=COALESCE($2,active),password_hash=COALESCE($3,password_hash),session_version=session_version+1 WHERE id=$4")
        .bind(patch.role).bind(patch.active).bind(hash).bind(id).execute(&mut *tx).await?;
    if changed.rows_affected()==0{return Err(ApiError::not_found());}
    if patch.active==Some(false) {
        sqlx::query("UPDATE api_keys SET revoked_at=now() WHERE user_id=$1").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"id":id,"updated":true})))
}
async fn protect_last_admin(tx:&mut sqlx::Transaction<'_,sqlx::Postgres>,id:Uuid)->ApiResult<()>{
    let row=sqlx::query("SELECT role,active FROM users WHERE id=$1")
        .bind(id).fetch_optional(&mut **tx).await?.ok_or_else(ApiError::not_found)?;
    if row.get::<String,_>("role")=="admin" && row.get::<bool,_>("active") {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM users WHERE role='admin' AND active=true")
            .fetch_one(&mut **tx).await?;
        if count <= 1 {return Err(ApiError::bad("at least one active administrator is required"));}
    }
    Ok(())
}
pub async fn delete_user(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let caller=require_user(&state,&headers).await?;
    admin_only(&caller)?;
    if caller.id==id{return Err(ApiError::bad("you cannot remove your own administrator account"));}
    let mut tx=state.pool.begin().await?;
    sqlx::query("SELECT id FROM site_settings WHERE id=TRUE FOR UPDATE")
        .fetch_one(&mut *tx).await?;
    protect_last_admin(&mut tx,id).await?;
    let result=sqlx::query("DELETE FROM users WHERE id=$1").bind(id).execute(&mut *tx).await?;
    if result.rows_affected()==0{return Err(ApiError::not_found());}
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
#[derive(Deserialize)]
pub struct NewKey {name:String,#[serde(default)] format:Option<String>}
pub async fn create_key(State(state):State<AppState>,headers:HeaderMap,Json(body):Json<NewKey>)->ApiResult<(StatusCode,Json<Value>)>{
    let user=require_user(&state,&headers).await?;
    if body.name.trim().is_empty() || body.name.len()>80 {return Err(ApiError::bad("key name must be 1 to 80 bytes"));}
    let prefix=match body.format.as_deref().unwrap_or("standard") {
        "standard"=>"mpk_","serverchan"=>"SCT",_=>return Err(ApiError::bad("format must be standard or serverchan"))
    };
    let secret=security::random_secret(prefix);
    let id=Uuid::new_v4();
    let short:String=secret.chars().take(10).collect();
    sqlx::query("INSERT INTO api_keys(id,user_id,name,key_hash,key_prefix) VALUES($1,$2,$3,$4,$5)")
        .bind(id).bind(user.id).bind(body.name).bind(security::hash_secret(&secret)).bind(short)
        .execute(&state.pool).await?;
    Ok((StatusCode::CREATED,Json(json!({"id":id,"key":secret,"note":"Save this key now; it will not be shown again."}))))
}
pub async fn list_keys(State(state):State<AppState>,headers:HeaderMap)->ApiResult<Json<Vec<Value>>>{
    let user=require_user(&state,&headers).await?;
    let rows=sqlx::query("SELECT id,name,key_prefix,created_at FROM api_keys WHERE user_id=$1 AND revoked_at IS NULL ORDER BY created_at DESC")
        .bind(user.id).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(|r|json!({
        "id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"prefix":r.get::<String,_>("key_prefix"),
        "created_at":r.get::<DateTime<Utc>,_>("created_at")
    })).collect()))
}
pub async fn delete_key(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let user=require_user(&state,&headers).await?;
    let rows=sqlx::query("UPDATE api_keys SET revoked_at=now() WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL")
        .bind(id).bind(user.id).execute(&state.pool).await?.rows_affected();
    if rows==0{return Err(ApiError::not_found());}
    Ok(Json(json!({"revoked":true})))
}
fn encrypt_provider(state:&AppState, input:&ProviderInput)->ApiResult<String>{
    input.validate()?;
    let secret=ProviderSecrets{settings:input.settings.clone(),delivery:input.delivery.clone()};
    let serialized=serde_json::to_string(&secret)?;
    state.crypto.encrypt(&serialized).map_err(ApiError::internal)
}
pub fn decrypt_provider(state:&AppState, raw:&str)->ApiResult<ProviderSecrets>{
    let serialized=state.crypto.decrypt(raw).map_err(ApiError::internal)?;
    serde_json::from_str(&serialized).map_err(ApiError::internal)
}
pub async fn list_providers(State(state):State<AppState>,headers:HeaderMap)->ApiResult<Json<Vec<Value>>>{
    let user=require_user(&state,&headers).await?;
    let rows=sqlx::query("SELECT p.id,p.name,p.kind,p.enabled,c.enabled AS available,p.encrypted_settings,p.created_at
          FROM providers p JOIN provider_catalog c ON c.kind=p.kind
          WHERE p.user_id=$1 ORDER BY p.created_at DESC")
        .bind(user.id).fetch_all(&state.pool).await?;
    let mut out=Vec::new();
    for r in rows {
        let secret=decrypt_provider(&state,r.get("encrypted_settings"))?;
        let kind:String=r.get("kind");
        let subscribed=match kind.as_str() {
            "telegram"=>secret.settings.get("chat_id").is_some_and(|v|!v.is_null() && v.to_string()!="\"\""),
            "wecom_app"=>secret.settings.get("to_user").and_then(Value::as_str).is_some_and(|s|!s.is_empty()),
            _=>true,
        };
        out.push(json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"kind":kind,
            "enabled":r.get::<bool,_>("enabled"),"available":r.get::<bool,_>("available"),"subscribed":subscribed,
            "delivery":match secret.delivery{DeliveryMode::Direct=>"direct",DeliveryMode::Proxy{..}=>"proxy"},
            "created_at":r.get::<DateTime<Utc>,_>("created_at")}));
    }
    Ok(Json(out))
}
pub async fn create_provider(State(state):State<AppState>,headers:HeaderMap,Json(input):Json<ProviderInput>)->ApiResult<(StatusCode,Json<Value>)>{
    let user=require_user(&state,&headers).await?;
    if !crate::catalog::is_enabled(&state.pool,&input.kind).await? {
        return Err(ApiError::bad("this provider type is not enabled by the site administrator"));
    }
    let sealed=encrypt_provider(&state,&input)?;
    let id=Uuid::new_v4();
    sqlx::query("INSERT INTO providers(id,user_id,name,kind,encrypted_settings,enabled) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(user.id).bind(input.name.trim()).bind(input.kind).bind(sealed).bind(input.enabled)
        .execute(&state.pool).await
        .map_err(|e|if e.as_database_error().is_some_and(|db|db.is_unique_violation()){ApiError::bad("provider name already exists")}else{ApiError::internal(e)})?;
    Ok((StatusCode::CREATED,Json(json!({"id":id}))))
}
pub async fn update_provider(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>,Json(input):Json<ProviderInput>)->ApiResult<Json<Value>>{
    let user=require_user(&state,&headers).await?;
    let current:Option<String>=sqlx::query_scalar("SELECT kind FROM providers WHERE id=$1 AND user_id=$2")
        .bind(id).bind(user.id).fetch_optional(&state.pool).await?;
    let Some(kind)=current else{return Err(ApiError::not_found());};
    if kind!=input.kind{return Err(ApiError::bad("provider type cannot be changed; create a new channel"));}
    let sealed=encrypt_provider(&state,&input)?;
    let rows=sqlx::query("UPDATE providers SET name=$1,encrypted_settings=$2,enabled=$3 WHERE id=$4 AND user_id=$5")
        .bind(input.name.trim()).bind(sealed).bind(input.enabled).bind(id).bind(user.id)
        .execute(&state.pool).await?.rows_affected();
    if rows==0{return Err(ApiError::not_found());}
    if input.enabled {
        sqlx::query("NOTIFY messagepusher_delivery").execute(&state.pool).await?;
    }
    Ok(Json(json!({"updated":true})))
}
pub async fn delete_provider(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let user=require_user(&state,&headers).await?;
    let mut tx=state.pool.begin().await?;
    sqlx::query("UPDATE deliveries SET status='failed',lease_until=NULL,last_error='The provider was removed' WHERE provider_id=$1 AND status IN ('pending','working')")
        .bind(id).execute(&mut *tx).await?;
    let rows=sqlx::query("DELETE FROM providers WHERE id=$1 AND user_id=$2")
        .bind(id).bind(user.id).execute(&mut *tx).await?.rows_affected();
    if rows==0{return Err(ApiError::not_found());}
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
pub async fn send_json(State(state):State<AppState>,headers:HeaderMap,Json(req):Json<SendRequest>)->ApiResult<(StatusCode,Json<Value>)>{
    let id=user_from_key(&state,bearer(&headers)?).await?;
    let idempotency=headers.get("idempotency-key").and_then(|v|v.to_str().ok());
    let event_id=enqueue(&state,id,req,idempotency).await?;
    Ok((StatusCode::ACCEPTED,Json(json!({"id":event_id,"status":"queued","status_url":format!("/v1/events/{}",event_id)}))))
}
pub async fn send_multipart(State(state):State<AppState>,headers:HeaderMap,mut data:Multipart)->ApiResult<(StatusCode,Json<Value>)>{
    let id=user_from_key(&state,bearer(&headers)?).await?;
    let idempotency=headers.get("idempotency-key").and_then(|v|v.to_str().ok()).map(str::to_owned);
    let mut req=SendRequest{title:String::new(),body:String::new(),provider_ids:None,attachments:vec![]};
    while let Some(field)=data.next_field().await.map_err(|e|ApiError::bad(e.to_string()))? {
        let name=field.name().unwrap_or("").to_owned();
        if name=="file" || name=="files" {
            let filename=field.file_name().unwrap_or("attachment.bin").to_owned();
            let content_type=field.content_type().unwrap_or("application/octet-stream").to_owned();
            let bytes=field.bytes().await.map_err(|e|ApiError::bad(e.to_string()))?;
            req.attachments.push(Attachment{filename,content_type,data_base64:STANDARD.encode(bytes)});
        } else {
            let text=field.text().await.map_err(|e|ApiError::bad(e.to_string()))?;
            match name.as_str(){
                "title"=>req.title=text,"body"|"desp"=>req.body=text,
                "provider_ids"=>req.provider_ids=Some(serde_json::from_str(&text).map_err(|_|ApiError::bad("provider_ids must be a JSON array of UUID strings"))?),
                _=>{},
            }
        }
    }
    let event_id=enqueue(&state,id,req,idempotency.as_deref()).await?;
    Ok((StatusCode::ACCEPTED,Json(json!({"id":event_id,"status":"queued"}))))
}
pub async fn enqueue_with_key(state:&AppState,key:&str,req:SendRequest)->ApiResult<Uuid>{
    let id=user_from_key(state,key).await?;
    enqueue(state,id,req,None).await
}
pub async fn enqueue(state:&AppState,user_id:Uuid,req:SendRequest,idempotency:Option<&str>)->ApiResult<Uuid>{
    req.validate()?;
    if let Some(k)=idempotency {if k.is_empty() || k.len()>128 {return Err(ApiError::bad("idempotency-key must be 1 to 128 bytes"));}}
    let mut tx=state.pool.begin().await?;
    let rows=sqlx::query("SELECT p.id,p.name FROM providers p JOIN provider_catalog c ON c.kind=p.kind
          WHERE p.user_id=$1 AND p.enabled=true AND c.enabled=true ORDER BY p.created_at")
        .bind(user_id).fetch_all(&mut *tx).await?;
    let mut ids:Vec<(Uuid,String)>=rows.into_iter().map(|r|(r.get("id"),r.get("name"))).collect();
    if let Some(selected)=req.provider_ids {
        if selected.is_empty(){return Err(ApiError::bad("provider_ids may not be empty"));}
        if selected.iter().any(|id|!ids.iter().any(|(pid,_)|pid==id)){return Err(ApiError::bad("provider_ids includes an unavailable provider"));}
        ids.retain(|(id,_)|selected.contains(id));
    }
    if ids.is_empty() {return Err(ApiError::bad("configure at least one enabled provider before sending"));}
    let id=Uuid::new_v4();
    let attachments=serde_json::to_value(&req.attachments)?;
    let inserted:Option<Uuid>=sqlx::query_scalar(
        "INSERT INTO events(id,user_id,title,body,attachments,idempotency_key) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(user_id,idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING RETURNING id")
        .bind(id).bind(user_id).bind(req.title).bind(req.body).bind(attachments).bind(idempotency)
        .fetch_optional(&mut *tx).await?;
    if inserted.is_none() {
        let previous:Uuid=sqlx::query_scalar("SELECT id FROM events WHERE user_id=$1 AND idempotency_key=$2")
            .bind(user_id).bind(idempotency).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        return Ok(previous);
    }
    let delivery_ids: Vec<Uuid> = (0..ids.len()).map(|_| Uuid::new_v4()).collect();
    let provider_ids: Vec<Uuid> = ids.iter().map(|(provider_id, _)| *provider_id).collect();
    let provider_names: Vec<String> = ids.into_iter().map(|(_, name)| name).collect();
    sqlx::query(
        "INSERT INTO deliveries (id, event_id, provider_id, provider_name)
         SELECT new_id, $1, provider_id, provider_name
         FROM UNNEST($2::uuid[], $3::uuid[], $4::text[])
         AS targets(new_id, provider_id, provider_name)"
    )
        .bind(id).bind(&delivery_ids).bind(&provider_ids).bind(&provider_names)
        .execute(&mut *tx).await?;
    // PostgreSQL sends NOTIFY only after the transaction commits.
    sqlx::query("NOTIFY messagepusher_delivery").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn list_events(State(state):State<AppState>,headers:HeaderMap)->ApiResult<Json<Vec<Value>>>{
    let user=require_user(&state,&headers).await?;
    let rows=sqlx::query(
        "WITH recent AS MATERIALIZED (
             SELECT id, title, created_at FROM events WHERE user_id=$1
             ORDER BY created_at DESC LIMIT 50
         )
         SELECT e.id,e.title,e.created_at,COUNT(d.id) total,
                COUNT(d.id) FILTER(WHERE d.status='sent') sent,
                COUNT(d.id) FILTER(WHERE d.status='failed') failed
         FROM recent e LEFT JOIN deliveries d ON d.event_id=e.id
         GROUP BY e.id,e.title,e.created_at ORDER BY e.created_at DESC")
        .bind(user.id).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(|r|json!({
        "id":r.get::<Uuid,_>("id"),"title":r.get::<String,_>("title"),
        "created_at":r.get::<DateTime<Utc>,_>("created_at"),
        "total":r.get::<i64,_>("total"),"sent":r.get::<i64,_>("sent"),"failed":r.get::<i64,_>("failed")
    })).collect()))
}
pub async fn event_detail(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let user=require_user(&state,&headers).await?;
    let row=sqlx::query(
        "SELECT title,body,created_at,
           COALESCE((
             SELECT jsonb_agg(jsonb_build_object(
                'filename', file->>'filename',
                'content_type', file->>'content_type'))
             FROM jsonb_array_elements(attachments) file
           ), '[]'::jsonb) AS attachment_details
         FROM events WHERE id=$1 AND user_id=$2"
    )
        .bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    let files: Value = row.get("attachment_details");
    let deliveries=sqlx::query(
        "SELECT d.id,d.status,d.attempts,d.last_error,d.sent_at,d.provider_name FROM deliveries d WHERE d.event_id=$1 ORDER BY d.provider_name")
        .bind(id).fetch_all(&state.pool).await?;
    Ok(Json(json!({
        "id":id,"title":row.get::<String,_>("title"),"body":row.get::<String,_>("body"),
        "attachments":files,
        "created_at":row.get::<DateTime<Utc>,_>("created_at"),
        "deliveries":deliveries.into_iter().map(|r|json!({
            "id":r.get::<Uuid,_>("id"),"provider":r.get::<String,_>("provider_name"),"status":r.get::<String,_>("status"),
            "attempts":r.get::<i32,_>("attempts"),"error":r.get::<Option<String>,_>("last_error"),
            "sent_at":r.get::<Option<DateTime<Utc>>,_>("sent_at")
        })).collect::<Vec<_>>()
    })))
}


#[derive(Deserialize)]
pub struct ProviderPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub settings: Option<Value>,
    pub delivery: Option<Value>,
}
fn visible_provider_settings(kind: &str, data: &Value) -> Value {
    let allowed: &[&str] = match kind {
        "telegram" => &["bot_username", "chat_id"],
        "wecom_app" => &["corp_id", "agent_id", "to_user"],
        "wecom_webhook" => &[],
        "email_smtp" => &["host", "port", "tls", "username", "from", "to"],
        "email_resend" => &["from", "to"],
        _ => &[],
    };
    let mut result = serde_json::Map::new();
    for key in allowed {
        if let Some(value) = data.get(*key) {
            result.insert((*key).into(), value.clone());
        }
    }
    Value::Object(result)
}
pub async fn provider_detail(
    State(state):State<AppState>, headers:HeaderMap, Path(id):Path<Uuid>
) -> ApiResult<Json<Value>> {
    let user = require_user(&state, &headers).await?;
    let row = sqlx::query(
        "SELECT name,kind,enabled,encrypted_settings FROM providers WHERE id=$1 AND user_id=$2"
    ).bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    let kind: String = row.get("kind");
    let saved = decrypt_provider(&state, row.get("encrypted_settings"))?;
    let stored_secrets: Vec<&str> = [
        "bot_token", "webhook_secret", "corp_secret",
        "webhook_key", "password", "api_key",
    ].iter().copied().filter(|key| saved.settings.get(*key).and_then(Value::as_str).is_some_and(|s|!s.is_empty())).collect();
    let route = match &saved.delivery {
        DeliveryMode::Direct => json!({"mode":"direct"}),
        DeliveryMode::Proxy{url,..} => json!({"mode":"proxy","url":url,"secret_saved":true}),
    };
    Ok(Json(json!({
        "id":id, "name":row.get::<String,_>("name"),
        "kind":kind, "enabled":row.get::<bool,_>("enabled"),
        "settings":visible_provider_settings(&kind,&saved.settings),
        "stored_secrets":stored_secrets,
        "delivery":route
    })))
}
pub async fn patch_provider(
    State(state):State<AppState>, headers:HeaderMap,Path(id):Path<Uuid>,Json(patch):Json<ProviderPatch>
) -> ApiResult<Json<Value>> {
    let user = require_user(&state,&headers).await?;
    if patch.name.is_none() && patch.enabled.is_none() && patch.settings.is_none() && patch.delivery.is_none() {
        return Err(ApiError::bad("no provider changes were provided"));
    }
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query(
        "SELECT name,kind,enabled,encrypted_settings FROM providers WHERE id=$1 AND user_id=$2 FOR UPDATE"
    ).bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::not_found)?;
    let mut saved = decrypt_provider(&state,row.get("encrypted_settings"))?;
    if let Some(fields) = patch.settings {
        let Value::Object(fields) = fields else { return Err(ApiError::bad("settings must be a JSON object")); };
        let Value::Object(ref mut current) = saved.settings else {return Err(ApiError::bad("invalid stored provider settings"));};
        for (key,value) in fields {
            if value.is_null() {current.remove(&key);} else {current.insert(key,value);}
        }
    }
    if let Some(route) = patch.delivery {
        let mode = route.get("mode").and_then(Value::as_str).ok_or_else(||ApiError::bad("delivery.mode is required"))?;
        saved.delivery = match mode {
            "direct" => DeliveryMode::Direct,
            "proxy" => {
                let prior = match &saved.delivery {
                    DeliveryMode::Proxy{url,secret} => Some((url.clone(),secret.clone())),
                    DeliveryMode::Direct => None,
                };
                let url = route.get("url").and_then(Value::as_str)
                    .filter(|s|!s.trim().is_empty()).map(str::to_owned)
                    .or_else(||prior.as_ref().map(|x|x.0.clone()))
                    .ok_or_else(||ApiError::bad("relay URL is required"))?;
                let secret = route.get("secret").and_then(Value::as_str)
                    .filter(|s|!s.is_empty()).map(str::to_owned)
                    .or_else(||prior.map(|x|x.1))
                    .ok_or_else(||ApiError::bad("relay secret is required"))?;
                DeliveryMode::Proxy{url,secret}
            },
            _ => return Err(ApiError::bad("unknown delivery mode")),
        };
    }
    let name = patch.name.unwrap_or_else(||row.get("name"));
    let enabled = patch.enabled.unwrap_or_else(||row.get("enabled"));
    let input = ProviderInput {
        name: name.clone(),kind:row.get("kind"),settings:saved.settings,
        delivery:saved.delivery,enabled
    };
    let sealed = encrypt_provider(&state,&input)?;
    sqlx::query("UPDATE providers SET name=$1,encrypted_settings=$2,enabled=$3 WHERE id=$4 AND user_id=$5")
        .bind(input.name.trim()).bind(sealed).bind(enabled).bind(id).bind(user.id)
        .execute(&mut *tx).await
        .map_err(|err|if err.as_database_error().is_some_and(|db|db.is_unique_violation()){
            ApiError::bad("provider name already exists")
        }else{ApiError::internal(err)})?;
    if enabled {
        sqlx::query("NOTIFY messagepusher_delivery").execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"id":id,"updated":true})))
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    #[test]
    fn safe_provider_view_never_exposes_credentials() {
        let source=json!({
            "bot_token":"private", "bot_username":"ExampleBot",
            "webhook_secret":"also-private", "chat_id":"12345"
        });
        let visible=visible_provider_settings("telegram",&source);
        assert_eq!(visible["bot_username"],"ExampleBot");
        assert_eq!(visible["chat_id"],"12345");
        assert!(visible.get("bot_token").is_none());
        assert!(visible.get("webhook_secret").is_none());
    }
}
