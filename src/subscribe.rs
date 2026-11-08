use crate::{api, error::{ApiError, ApiResult}, models::{DeliveryMode, ProviderSecrets, RelayBatch, RelayItem, RelayResponse}, security, AppState};
use axum::{extract::{Path,Query,State},http::HeaderMap, response::Html,Json};
use chrono::{DateTime,Utc};
use serde::Deserialize;
use serde_json::{json,Value};
use sqlx::Row;
use uuid::Uuid;

async fn own_provider(state:&AppState, headers:&HeaderMap,id:Uuid,kind:&str)->ApiResult<ProviderSecrets>{
    let user=api::require_user(state,headers).await?;
    let row=sqlx::query("SELECT encrypted_settings,kind FROM providers WHERE id=$1 AND user_id=$2 AND enabled=true")
        .bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    if row.get::<String,_>("kind")!=kind{return Err(ApiError::bad("provider has the wrong type"));}
    api::decrypt_provider(state,row.get("encrypted_settings"))
}
async fn create_subscription(state:&AppState,headers:&HeaderMap,id:Uuid,kind:&str)->ApiResult<String>{
    let user=api::require_user(state,headers).await?;
    let code=security::random_secret("mp");
    sqlx::query("INSERT INTO subscriptions(id,user_id,provider_id,kind,code_hash,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '15 minutes')")
        .bind(Uuid::new_v4()).bind(user.id).bind(id).bind(kind).bind(security::hash_secret(&code))
        .execute(&state.pool).await?;
    Ok(code)
}
pub async fn telegram_subscribe(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let provider=own_provider(&state,&headers,id,"telegram").await?;
    let username=provider.settings.get("bot_username").and_then(Value::as_str)
        .ok_or_else(||ApiError::bad("set bot_username in this Telegram provider"))?;
    if username.trim_start_matches('@').chars().any(|c|!c.is_ascii_alphanumeric() && c!='_'){
        return Err(ApiError::bad("invalid Telegram bot username"));
    }
    let code=create_subscription(&state,&headers,id,"telegram").await?;
    Ok(Json(json!({"url":format!("https://t.me/{}?start={}",username.trim_start_matches('@'),code),"expires_in_seconds":900})))
}
pub async fn telegram_set_webhook(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let provider=own_provider(&state,&headers,id,"telegram").await?;
    if !state.public_url.starts_with("https://"){return Err(ApiError::bad("PUBLIC_BASE_URL must be a publicly accessible HTTPS address"));}
    let token=provider.settings.get("bot_token").and_then(Value::as_str).ok_or_else(||ApiError::bad("missing bot_token"))?;
    let secret=provider.settings.get("webhook_secret").and_then(Value::as_str).filter(|s|s.len()>=16)
        .ok_or_else(||ApiError::bad("set webhook_secret (at least 16 characters) in the provider"))?;
    let url=format!("{}/hooks/telegram/{}",state.public_url,id);
    let result=match provider.delivery {
        DeliveryMode::Direct=>{
            let response=state.http.post(format!("https://api.telegram.org/bot{}/setWebhook",token))
                .json(&json!({"url":url,"secret_token":secret})).send().await.map_err(ApiError::internal)?;
            let value:Value=response.json().await.map_err(ApiError::internal)?;
            if value.get("ok").and_then(Value::as_bool)!=Some(true){return Err(ApiError::bad("Telegram rejected the webhook configuration"));}
            true
        }
        DeliveryMode::Proxy{url:relay_url,secret:relay_secret}=>{
            let delivery_id=Uuid::new_v4();
            let batch=RelayBatch{items:vec![RelayItem{id:delivery_id,kind:"telegram_webhook_setup".into(),settings:json!({"bot_token":token,"webhook_secret":secret}),
                title:url,body:String::new(),attachments:vec![]}]};
            let bytes=serde_json::to_vec(&batch).map_err(ApiError::internal)?;
            let time=Utc::now().timestamp();
            let response=state.http.post(relay_url).header("x-mp-timestamp",time.to_string())
                .header("x-mp-signature",security::hmac_hex(&relay_secret,time,&bytes))
                .header("content-type","application/json").body(bytes).send().await.map_err(ApiError::internal)?;
            if !response.status().is_success(){return Err(ApiError::bad("relay refused the webhook request"));}
            let value:RelayResponse=response.json().await.map_err(ApiError::internal)?;
            value.results.iter().any(|r|r.id==delivery_id && r.ok)
        }
    };
    if !result {return Err(ApiError::bad("Telegram webhook setup failed"));}
    Ok(Json(json!({"configured":true})))
}
pub async fn telegram_hook(State(state):State<AppState>,Path(id):Path<Uuid>,headers:HeaderMap,Json(data):Json<Value>)->ApiResult<Json<Value>>{
    let row=sqlx::query("SELECT encrypted_settings FROM providers WHERE id=$1 AND kind='telegram' AND enabled=true")
        .bind(id).fetch_optional(&state.pool).await?.ok_or_else(ApiError::not_found)?;
    let provider=api::decrypt_provider(&state,row.get("encrypted_settings"))?;
    let expected=provider.settings.get("webhook_secret").and_then(Value::as_str)
        .ok_or_else(ApiError::unauthorized)?;
    let given=headers.get("x-telegram-bot-api-secret-token").and_then(|v|v.to_str().ok()).unwrap_or("");
    if !security::secure_equal(expected,given){return Err(ApiError::unauthorized());}
    let Some(message)=data.get("message") else{return Ok(Json(json!({"ok":true})));};
    let Some(text)=message.get("text").and_then(Value::as_str) else{return Ok(Json(json!({"ok":true})));};
    let Some(code)=text.split_whitespace().nth(1).filter(|_|text.starts_with("/start ")) else{return Ok(Json(json!({"ok":true})));};
    if message.pointer("/chat/type").and_then(Value::as_str)!=Some("private"){return Ok(Json(json!({"ok":true}))); }
    let Some(chat)=message.pointer("/chat/id") else{return Ok(Json(json!({"ok":true})));};
    let chat_id=chat.to_string();
    let mut tx=state.pool.begin().await?;
    let pending=sqlx::query("SELECT id,expires_at FROM subscriptions WHERE provider_id=$1 AND kind='telegram' AND code_hash=$2 FOR UPDATE")
        .bind(id).bind(security::hash_secret(code)).fetch_optional(&mut *tx).await?;
    if let Some(pending)=pending{
        let expires:DateTime<Utc>=pending.get("expires_at");
        if expires>Utc::now(){
            let mut settings=provider;
            settings.settings["chat_id"]=json!(chat_id);
            let sealed=state.crypto.encrypt(&serde_json::to_string(&settings)?).map_err(ApiError::internal)?;
            sqlx::query("UPDATE providers SET encrypted_settings=$1 WHERE id=$2").bind(sealed).bind(id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM subscriptions WHERE id=$1").bind(pending.get::<Uuid,_>("id")).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn wecom_subscribe(State(state):State<AppState>,headers:HeaderMap,Path(id):Path<Uuid>)->ApiResult<Json<Value>>{
    let provider=own_provider(&state,&headers,id,"wecom_app").await?;
    if !state.public_url.starts_with("https://"){return Err(ApiError::bad("PUBLIC_BASE_URL must use HTTPS"));}
    let corp=provider.settings.get("corp_id").and_then(Value::as_str).ok_or_else(||ApiError::bad("missing corp_id"))?;
    let agent=provider.settings.get("agent_id").map(|v|if let Some(s)=v.as_str(){s.to_string()}else{v.to_string()})
        .ok_or_else(||ApiError::bad("missing agent_id"))?;
    let code=create_subscription(&state,&headers,id,"wecom").await?;
    let mut url=url::Url::parse("https://open.weixin.qq.com/connect/oauth2/authorize").map_err(ApiError::internal)?;
    url.query_pairs_mut().append_pair("appid",corp)
        .append_pair("redirect_uri",&format!("{}/hooks/wecom",state.public_url))
        .append_pair("response_type","code").append_pair("scope","snsapi_base")
        .append_pair("agentid",&agent).append_pair("state",&code);
    url.set_fragment(Some("wechat_redirect"));
    Ok(Json(json!({"url":url.to_string(),"expires_in_seconds":900})))
}
#[derive(Deserialize)]
pub struct OAuthCallback{code:String,state:String}
pub async fn wecom_callback(State(state):State<AppState>,Query(request):Query<OAuthCallback>)->ApiResult<Html<&'static str>>{
    let mut tx=state.pool.begin().await?;
    let pending=sqlx::query(
        "SELECT s.id,s.provider_id,s.expires_at,p.encrypted_settings FROM subscriptions s
         JOIN providers p ON p.id=s.provider_id WHERE s.kind='wecom' AND s.code_hash=$1 FOR UPDATE OF s")
        .bind(security::hash_secret(&request.state)).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::unauthorized)?;
    if pending.get::<DateTime<Utc>,_>("expires_at")<=Utc::now(){return Err(ApiError::bad("authorization link has expired"));}
    let id:Uuid=pending.get("provider_id");
    let mut provider=api::decrypt_provider(&state,pending.get("encrypted_settings"))?;
    let corp=provider.settings.get("corp_id").and_then(Value::as_str).ok_or_else(ApiError::unauthorized)?;
    let secret=provider.settings.get("corp_secret").and_then(Value::as_str).ok_or_else(ApiError::unauthorized)?;
    let token:Value=state.http.get("https://qyapi.weixin.qq.com/cgi-bin/gettoken")
        .query(&[("corpid",corp),("corpsecret",secret)]).send().await.map_err(ApiError::internal)?
        .json().await.map_err(ApiError::internal)?;
    if token.get("errcode").and_then(Value::as_i64)!=Some(0){return Err(ApiError::bad("WeCom access token request failed"));}
    let token=token.get("access_token").and_then(Value::as_str).ok_or_else(ApiError::unauthorized)?;
    let response:Value=state.http.get("https://qyapi.weixin.qq.com/cgi-bin/auth/getuserinfo")
        .query(&[("access_token",token),("code",request.code.as_str())])
        .send().await.map_err(ApiError::internal)?.json().await.map_err(ApiError::internal)?;
    if response.get("errcode").and_then(Value::as_i64)!=Some(0){return Err(ApiError::bad("WeCom could not verify the member"));}
    let member=response.get("userid").and_then(Value::as_str).ok_or_else(||ApiError::bad("WeCom did not return an enterprise member ID"))?;
    provider.settings["to_user"]=json!(member);
    let sealed=state.crypto.encrypt(&serde_json::to_string(&provider)?).map_err(ApiError::internal)?;
    sqlx::query("UPDATE providers SET encrypted_settings=$1 WHERE id=$2").bind(sealed).bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM subscriptions WHERE id=$1").bind(pending.get::<Uuid,_>("id")).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Html("<!doctype html><html lang='zh-CN'><meta charset='utf-8'><title>授权成功</title><body><h2>企业微信授权成功</h2><p>现在可以关闭此页面，返回 MessagePusher。</p></body></html>"))
}
