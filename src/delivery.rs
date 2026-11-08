use crate::{api, models::{Attachment, DeliveryMode, ProviderSecrets, RelayBatch, RelayItem, RelayResponse, MAX_PROXY_BATCH_BYTES}, security, AppState};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use lettre::{message::{header::ContentType, Attachment as EmailAttachment, Mailbox, MultiPart, SinglePart}, transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use reqwest::multipart::{Form, Part};
use serde_json::{json, Value};
use sqlx::{postgres::PgListener, Row};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, time::Duration};
use tokio::task::JoinSet;
use uuid::Uuid;

#[derive(Clone)]
struct Job {
    item: RelayItem,
    attempts: i32,
    delivery: DeliveryMode,
}

const DELIVERY_CHANNEL: &str = "messagepusher_delivery";
const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(60);
const CLAIM_BATCH_SIZE: usize = 16;
const SEND_TIMEOUT: Duration = Duration::from_secs(55);

async fn connect_listener(state: &AppState) -> Result<PgListener> {
    let mut listener = PgListener::connect_with(&state.pool).await?;
    listener.listen(DELIVERY_CHANNEL).await?;
    Ok(listener)
}

async fn next_wakeup(state: &AppState) -> Result<Duration> {
    let next_due: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT MIN(due_at) FROM (
            SELECT MIN(d.next_attempt_at) AS due_at
            FROM deliveries d
            JOIN providers p ON p.id=d.provider_id
            JOIN provider_catalog c ON c.kind=p.kind
            WHERE d.status='pending' AND p.enabled=true AND c.enabled=true
            UNION ALL
            SELECT MIN(d.lease_until) AS due_at
            FROM deliveries d
            JOIN providers p ON p.id=d.provider_id
            JOIN provider_catalog c ON c.kind=p.kind
            WHERE d.status='working' AND p.enabled=true AND c.enabled=true
         ) next_jobs"
    ).fetch_one(&state.pool).await?;
    let wait = next_due
        .as_ref()
        .and_then(|due| due.signed_duration_since(Utc::now()).to_std().ok())
        .unwrap_or_else(|| if next_due.is_some() {
            Duration::from_secs(1)
        } else {
            RECONCILIATION_INTERVAL
        });
    Ok(wait.max(Duration::from_secs(1)).min(RECONCILIATION_INTERVAL))
}

pub async fn worker(state: AppState, concurrency: usize) {
    let mut listener = loop {
        match connect_listener(&state).await {
            Ok(listener) => break listener,
            Err(error) => {
                tracing::warn!(%error, "Unable to subscribe to PostgreSQL notifications");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    };

    // Waiting for a notification starts after LISTEN is registered, so starting
    // the process while another process inserts work cannot leave a gap.
    let mut tasks = JoinSet::<usize>::new();
    let mut in_flight = 0usize;

    loop {
        // Refill capacity when any task finishes. A slow provider does not delay
        // unrelated deliveries that are ready in PostgreSQL.
        while in_flight < concurrency {
            let available = (concurrency - in_flight).min(CLAIM_BATCH_SIZE);
            match claim_jobs(&state, available as i64).await {
                Ok(jobs) if !jobs.is_empty() => {
                    in_flight += jobs.len();
                    schedule_jobs(&state, jobs, &mut tasks);
                }
                Ok(_) => break,
                Err(error) => {
                    tracing::error!(%error, "Unable to claim pending deliveries");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    break;
                }
            }
        }

        if in_flight >= concurrency {
            if let Some(joined) = tasks.join_next().await {
                update_in_flight(&mut in_flight, joined);
            }
            continue;
        }

        let wait = match next_wakeup(&state).await {
            Ok(delay) => delay,
            Err(error) => {
                tracing::warn!(%error, "Unable to calculate next delivery time");
                Duration::from_secs(3)
            }
        };
        tokio::select! {
            joined = tasks.join_next(), if in_flight > 0 => {
                if let Some(result) = joined {
                    update_in_flight(&mut in_flight, result);
                }
            }
            result = listener.recv() => {
                if let Err(error) = result {
                    tracing::warn!(%error, "PostgreSQL notification connection interrupted");
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

fn update_in_flight(in_flight: &mut usize, joined: Result<usize, tokio::task::JoinError>) {
    match joined {
        Ok(count) => *in_flight = in_flight.saturating_sub(count),
        Err(error) => {
            tracing::error!(%error, "A delivery task stopped unexpectedly");
            // Recover bounded capacity even if a task panicked. PostgreSQL
            // eventually reclaims unfinished work through expired leases.
            *in_flight = in_flight.saturating_sub(1);
        }
    }
}

async fn claim_jobs(state: &AppState, limit: i64) -> Result<Vec<Job>> {
    // The update and the joined data are returned in one database round trip.
    let rows = sqlx::query(
        "WITH selected AS MATERIALIZED (
            SELECT d.id FROM deliveries d
            JOIN providers p ON p.id=d.provider_id
            JOIN provider_catalog c ON c.kind=p.kind
            WHERE p.enabled=true AND c.enabled=true
              AND (
                (d.status='pending' AND d.next_attempt_at<=now())
                OR (d.status='working' AND d.lease_until<now())
              )
            ORDER BY d.created_at
            LIMIT $1 FOR UPDATE OF d SKIP LOCKED
         ), claimed AS (
            UPDATE deliveries d
            SET status='working',
                lease_until=now()+interval '75 seconds',
                attempts=d.attempts+1
            FROM selected WHERE d.id=selected.id
            RETURNING d.id,d.attempts,d.event_id,d.provider_id
         )
         SELECT claimed.id,claimed.attempts,
                p.kind,p.encrypted_settings,
                e.title,e.body,e.attachments
         FROM claimed
         JOIN providers p ON p.id=claimed.provider_id
         JOIN events e ON e.id=claimed.event_id"
    ).bind(limit).fetch_all(&state.pool).await?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        let parsed: Result<Job> = (|| {
            let sealed: &str = row.get("encrypted_settings");
            let secrets: ProviderSecrets = api::decrypt_provider(state, sealed)
                .map_err(|error| anyhow!(error.1))?;
            let attachments: Vec<Attachment> =
                serde_json::from_value(row.get::<Value, _>("attachments"))?;
            Ok(Job {
                item: RelayItem {
                    id, kind: row.get("kind"), settings: secrets.settings,
                    title: row.get("title"), body: row.get("body"), attachments,
                },
                attempts,
                delivery: secrets.delivery,
            })
        })();
        match parsed {
            Ok(job) => jobs.push(job),
            Err(error) => {
                // A corrupted configuration cannot succeed on retry.
                if let Err(db_error) = sqlx::query(
                    "UPDATE deliveries
                     SET status='failed',lease_until=NULL,last_error=$2
                     WHERE id=$1 AND status='working' AND attempts=$3"
                )
                    .bind(id).bind(error.to_string().chars().take(300).collect::<String>())
                    .bind(attempts).execute(&state.pool).await {
                    tracing::error!(%db_error, %id, "Unable to mark invalid delivery");
                }
            }
        }
    }
    Ok(jobs)
}

fn schedule_jobs(state: &AppState, jobs: Vec<Job>, tasks: &mut JoinSet<usize>) {
    let mut grouped: HashMap<(String, String), Vec<Job>> = HashMap::new();
    for job in jobs {
        match &job.delivery {
            DeliveryMode::Direct => {
                let service = state.clone();
                tasks.spawn(async move {
                    let result = tokio::time::timeout(
                        SEND_TIMEOUT, send_direct(&service, &job.item)
                    ).await
                        .map_err(|_| "provider deadline exceeded".to_owned())
                        .and_then(|result| result.map_err(|error| error.to_string()));
                    finish(&service, job.item.id, job.attempts, result).await;
                    1
                });
            }
            DeliveryMode::Proxy { url, secret } => {
                grouped.entry((url.clone(), secret.clone())).or_default().push(job);
            }
        }
    }
    for ((url, secret), group) in grouped {
        for batch in split_proxy_batches(group) {
            let count = batch.len();
            let service = state.clone();
            let url = url.clone();
            let secret = secret.clone();
            tasks.spawn(async move {
                if count == 1 && serde_json::to_vec(&RelayBatch {
                    items: batch.iter().map(|job| job.item.clone()).collect(),
                }).map_or(true, |data| data.len() > MAX_PROXY_BATCH_BYTES) {
                    let job = &batch[0];
                    finish(&service, job.item.id, job.attempts,
                        Err("message exceeds relay request limit; reduce attachments".into())
                    ).await;
                } else {
                    send_proxy_group(&service, &url, &secret, batch).await;
                }
                count
            });
        }
    }
}

fn split_proxy_batches(jobs: Vec<Job>) -> Vec<Vec<Job>> {
    // The encoded size of each item is calculated once. Concatenation costs
    // only the JSON-array comma; it does not reserialize previous items.
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let empty_size = b"{\"items\":[]}".len();
    let mut used = empty_size;

    for job in jobs {
        let entry = serde_json::to_vec(&job.item).map_or(usize::MAX, |data| data.len());
        let comma = usize::from(!current.is_empty());
        if !current.is_empty() && used.saturating_add(entry).saturating_add(comma) > MAX_PROXY_BATCH_BYTES {
            batches.push(std::mem::take(&mut current));
            used = empty_size;
        }
        used = used.saturating_add(entry).saturating_add(usize::from(!current.is_empty()));
        current.push(job);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

fn retry_delay_seconds(attempts: i32) -> i32 {
    (20 * 2_i32.pow(attempts.max(0).min(6) as u32)).min(1800)
}

async fn finish(state: &AppState, id: Uuid, attempts: i32, result: Result<(), String>) {
    let updated = match result {
        Ok(()) => sqlx::query(
            "UPDATE deliveries SET status='sent', lease_until=NULL,
             last_error=NULL, sent_at=now()
             WHERE id=$1 AND status='working' AND attempts=$2"
        ).bind(id).bind(attempts).execute(&state.pool).await,
        Err(error) if attempts >= 5 => sqlx::query(
            "UPDATE deliveries SET status='failed',lease_until=NULL,last_error=$3
             WHERE id=$1 AND status='working' AND attempts=$2"
        ).bind(id).bind(attempts)
            .bind(error.chars().take(300).collect::<String>())
            .execute(&state.pool).await,
        Err(error) => sqlx::query(
            "UPDATE deliveries SET status='pending',lease_until=NULL,last_error=$3,
             next_attempt_at=now()+($4 * interval '1 second')
             WHERE id=$1 AND status='working' AND attempts=$2"
        ).bind(id).bind(attempts)
            .bind(error.chars().take(300).collect::<String>())
            .bind(retry_delay_seconds(attempts)).execute(&state.pool).await,
    };
    if let Err(error) = updated {
        tracing::error!(%error, %id, "Unable to save delivery result");
    }
}

async fn save_proxy_results(state: &AppState, jobs: &[Job], outcomes: Vec<Result<(), String>>) {
    let mut ids = Vec::with_capacity(jobs.len());
    let mut attempts = Vec::with_capacity(jobs.len());
    let mut statuses = Vec::with_capacity(jobs.len());
    let mut errors: Vec<Option<String>> = Vec::with_capacity(jobs.len());
    let mut delays = Vec::with_capacity(jobs.len());

    for (job, result) in jobs.iter().zip(&outcomes) {
        ids.push(job.item.id);
        attempts.push(job.attempts);
        match result {
            Ok(()) => {
                statuses.push("sent".to_owned());
                errors.push(None);
                delays.push(0);
            }
            Err(error) => {
                statuses.push(if job.attempts >= 5 { "failed" } else { "pending" }.to_owned());
                errors.push(Some(error.chars().take(300).collect()));
                delays.push(retry_delay_seconds(job.attempts));
            }
        }
    }
    // All outcomes from one relay response are committed in one SQL statement.
    let saved = sqlx::query(
        "UPDATE deliveries d
         SET status=results.status,
             lease_until=NULL,
             last_error=results.error,
             sent_at=CASE WHEN results.status='sent' THEN now() ELSE d.sent_at END,
             next_attempt_at=CASE WHEN results.status='pending'
                 THEN now()+results.delay * interval '1 second'
                 ELSE d.next_attempt_at END
         FROM UNNEST($1::uuid[], $2::int4[], $3::text[], $4::text[], $5::int4[])
           AS results(id,attempts,status,error,delay)
         WHERE d.id=results.id
           AND d.status='working' AND d.attempts=results.attempts"
    )
        .bind(&ids).bind(&attempts).bind(&statuses).bind(&errors).bind(&delays)
        .execute(&state.pool).await;
    if let Err(error) = saved {
        tracing::error!(%error, "Unable to save relay batch results");
        for (job, result) in jobs.iter().zip(outcomes) {
            finish(state, job.item.id, job.attempts, result).await;
        }
    }
}

async fn send_proxy_group(state: &AppState, url: &str, secret: &str, jobs: Vec<Job>) {
    let payload = RelayBatch { items: jobs.iter().map(|job| job.item.clone()).collect() };
    let response: Result<RelayResponse> = async {
        let bytes = serde_json::to_vec(&payload)?;
        let timestamp = chrono::Utc::now().timestamp();
        let signature = security::hmac_hex(secret, timestamp, &bytes);
        let result = state.http.post(url)
            .header("x-mp-timestamp", timestamp.to_string())
            .header("x-mp-signature", signature)
            .header("content-type", "application/json")
            .body(bytes).send().await?;
        if !result.status().is_success() {
            bail!("relay returned HTTP {}", result.status().as_u16());
        }
        result.json().await.context("invalid relay response")
    }.await;

    let outcomes = match response {
        Ok(response) => {
            let lookup: HashMap<Uuid, _> = response.results.into_iter()
                .map(|result| (result.id, result)).collect();
            jobs.iter().map(|job| {
                lookup.get(&job.item.id).map(|result| {
                    if result.ok { Ok(()) } else {
                        Err(result.error.clone().unwrap_or_else(|| "provider rejected the request".into()))
                    }
                }).unwrap_or_else(|| Err("relay omitted the delivery result".into()))
            }).collect()
        }
        Err(error) => {
            let message = format!("relay request failed: {error}");
            jobs.iter().map(|_| Err(message.clone())).collect()
        }
    };
    save_proxy_results(state, &jobs, outcomes).await;
}
fn setting<'a>(settings:&'a Value,key:&str)->Result<&'a str> {
    settings.get(key).and_then(Value::as_str).filter(|v|!v.is_empty())
        .ok_or_else(||anyhow!("missing provider setting: {}",key))
}
fn chat_id(settings:&Value)->Result<String> {
    let value=settings.get("chat_id").ok_or_else(||anyhow!("Telegram bot has not been subscribed to; open the subscription link"))?;
    match value {
        Value::String(s) if !s.is_empty()=>Ok(s.clone()),
        Value::Number(n)=>Ok(n.to_string()),
        _=>bail!("invalid Telegram chat ID"),
    }
}
async fn check_json(response:reqwest::Response,success_field:&str)->Result<Value>{
    let status=response.status();
    if !status.is_success(){bail!("provider returned HTTP {}",status.as_u16());}
    let data:Value=response.json().await?;
    let ok=match success_field {
        "ok"=>data.get("ok").and_then(Value::as_bool)==Some(true),
        "errcode"=>data.get("errcode").and_then(Value::as_i64)==Some(0),
        _=>true,
    };
    if !ok {
        let code=data.get("errcode").or_else(||data.get("error_code")).and_then(Value::as_i64).unwrap_or(-1);
        bail!("provider rejected the request (code {})",code);
    }
    Ok(data)
}
fn decoded(file:&Attachment)->Result<Vec<u8>> {Ok(STANDARD.decode(&file.data_base64)?)}
fn form_file(file:&Attachment)->Result<Form>{
    let part=Part::bytes(decoded(file)?).file_name(file.filename.clone())
        .mime_str(&file.content_type)?;
    Ok(Form::new().part("media",part))
}
pub async fn send_direct(state:&AppState,item:&RelayItem)->Result<()>{
    match item.kind.as_str(){
        "telegram"=>send_telegram(state,item).await,
        "wecom_app"=>send_wecom_app(state,item).await,
        "wecom_webhook"=>send_wecom_webhook(state,item).await,
        "email_resend"=>send_resend(state,item).await,
        "email_smtp"=>send_smtp(item).await,
        _=>bail!("unknown provider type"),
    }
}
async fn send_telegram(state:&AppState,item:&RelayItem)->Result<()>{
    let bot=setting(&item.settings,"bot_token")?;
    let chat=chat_id(&item.settings)?;
    if bot.split_once(':').is_none_or(|(id,secret)| {
        id.is_empty() || !id.bytes().all(|c|c.is_ascii_digit())
            || secret.is_empty()
            || !secret.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
    }) { bail!("invalid Telegram bot token"); }
    let text=if item.title.is_empty(){item.body.clone()}else{format!("{}\n\n{}",item.title,item.body)};
    if !text.is_empty() {
        for chunk in split_chars(&text,4000) {
            let response=state.http.post(format!("https://api.telegram.org/bot{}/sendMessage",bot))
                .json(&json!({"chat_id":chat,"text":chunk})).send().await?;
            check_json(response,"ok").await?;
        }
    }
    for file in &item.attachments {
        let part=Part::bytes(decoded(file)?).file_name(file.filename.clone()).mime_str(&file.content_type)?;
        let response=state.http.post(format!("https://api.telegram.org/bot{}/sendDocument",bot))
            .multipart(Form::new().text("chat_id",chat.clone()).part("document",part)).send().await?;
        check_json(response,"ok").await?;
    }
    Ok(())
}
fn split_chars(input:&str,maximum:usize)->Vec<String>{
    let mut parts=Vec::new();
    let mut current=String::new();
    let mut count=0;
    for ch in input.chars() {
        if count>=maximum {
            parts.push(std::mem::take(&mut current));
            count=0;
        }
        current.push(ch);
        count+=1;
    }
    if !current.is_empty(){parts.push(current);}
    parts
}
async fn wecom_token(state:&AppState,settings:&Value)->Result<String>{
    let response=state.http.get("https://qyapi.weixin.qq.com/cgi-bin/gettoken")
        .query(&[("corpid",setting(settings,"corp_id")?),("corpsecret",setting(settings,"corp_secret")?)])
        .send().await?;
    let json=check_json(response,"errcode").await?;
    Ok(json.get("access_token").and_then(Value::as_str).ok_or_else(||anyhow!("WeCom access token missing"))?.into())
}
async fn send_wecom_app(state:&AppState,item:&RelayItem)->Result<()>{
    let token=wecom_token(state,&item.settings).await?;
    let recipient=setting(&item.settings,"to_user").context("WeCom user has not completed authorization")?;
    let agent=match item.settings.get("agent_id") {
        Some(Value::Number(n))=>n.as_i64().unwrap_or_default(),
        Some(Value::String(s))=>s.parse::<i64>()?,
        _=>bail!("invalid WeCom agent_id"),
    };
    let send_url=format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={}",token);
    let text=format!("{}\n{}",item.title,item.body);
    let msg=json!({"touser":recipient,"msgtype":"text","agentid":agent,"text":{"content":text}});
    check_json(state.http.post(&send_url).json(&msg).send().await?,"errcode").await?;
    for file in &item.attachments {
        let upload=format!("https://qyapi.weixin.qq.com/cgi-bin/media/upload?access_token={}&type=file",token);
        let json=check_json(state.http.post(&upload).multipart(form_file(file)?).send().await?,"errcode").await?;
        let media_id=json.get("media_id").and_then(Value::as_str).ok_or_else(||anyhow!("WeCom media upload returned no id"))?;
        let msg=json!({"touser":recipient,"msgtype":"file","agentid":agent,"file":{"media_id":media_id}});
        check_json(state.http.post(&send_url).json(&msg).send().await?,"errcode").await?;
    }
    Ok(())
}
async fn send_wecom_webhook(state:&AppState,item:&RelayItem)->Result<()>{
    let key=setting(&item.settings,"webhook_key")?;
    let mut send=url::Url::parse("https://qyapi.weixin.qq.com/cgi-bin/webhook/send")?;
    send.query_pairs_mut().append_pair("key",key);
    let msg=json!({"msgtype":"text","text":{"content":format!("{}\n{}",item.title,item.body)}});
    check_json(state.http.post(send).json(&msg).send().await?,"errcode").await?;
    for file in &item.attachments {
        let mut upload=url::Url::parse("https://qyapi.weixin.qq.com/cgi-bin/webhook/upload_media")?;
        upload.query_pairs_mut().append_pair("key",key).append_pair("type","file");
        let json=check_json(state.http.post(upload).multipart(form_file(file)?).send().await?,"errcode").await?;
        let media_id=json.get("media_id").and_then(Value::as_str).ok_or_else(||anyhow!("WeCom webhook media id missing"))?;
        let msg=json!({"msgtype":"file","file":{"media_id":media_id}});
        let mut send=url::Url::parse("https://qyapi.weixin.qq.com/cgi-bin/webhook/send")?;
        send.query_pairs_mut().append_pair("key",key);
        check_json(state.http.post(send).json(&msg).send().await?,"errcode").await?;
    }
    Ok(())
}
fn email_recipients(settings:&Value)->Result<Vec<String>>{
    let value=settings.get("to").ok_or_else(||anyhow!("email recipient missing"))?;
    let recipients=match value{
        Value::String(s)=>vec![s.clone()],
        Value::Array(arr)=>arr.iter().map(|v|v.as_str().map(str::to_owned).ok_or_else(||anyhow!("invalid email recipient"))).collect::<Result<Vec<_>>>()?,
        _=>bail!("invalid recipient list"),
    };
    if recipients.is_empty() || recipients.len()>30 {bail!("email requires 1 to 30 recipients");}
    Ok(recipients)
}
async fn send_resend(state:&AppState,item:&RelayItem)->Result<()>{
    let attachments:Vec<Value>=item.attachments.iter().map(|file|json!({
        "filename":file.filename,"content":file.data_base64,"content_type":file.content_type
    })).collect();
    let msg=json!({"from":setting(&item.settings,"from")?,
        "to":email_recipients(&item.settings)?,
        "subject":if item.title.is_empty(){"Notification"}else{&item.title},
        "text":if item.body.is_empty(){" "}else{&item.body},
        "attachments":attachments
    });
    let response=state.http.post("https://api.resend.com/emails")
        .bearer_auth(setting(&item.settings,"api_key")?).json(&msg).send().await?;
    if !response.status().is_success(){bail!("Resend returned HTTP {}",response.status().as_u16());}
    Ok(())
}
async fn send_smtp(item:&RelayItem)->Result<()>{
    let host=setting(&item.settings,"host")?;
    let from:Mailbox=setting(&item.settings,"from")?.parse()?;
    let port=item.settings.get("port").and_then(Value::as_u64).unwrap_or(587) as u16;
    let tls=item.settings.get("tls").and_then(Value::as_str).unwrap_or("starttls");
    let mut builder=match tls{
        "implicit"=>AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
        "starttls"=>AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
        "none"=>AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
        _=>bail!("smtp tls must be implicit, starttls or none"),
    };
    builder=builder.port(port);
    if let (Some(username),Some(password))=(
        item.settings.get("username").and_then(Value::as_str),
        item.settings.get("password").and_then(Value::as_str)
    ){
        builder=builder.credentials(Credentials::new(username.into(),password.into()));
    }
    let mut message=Message::builder().from(from).subject(if item.title.is_empty(){"Notification"}else{&item.title});
    for email in email_recipients(&item.settings)? {message=message.to(email.parse()?);}
    let result=if item.attachments.is_empty(){
        message.body(item.body.clone())?
    }else{
        let mut multi=MultiPart::mixed().singlepart(SinglePart::plain(item.body.clone()));
        for file in &item.attachments{
            let mime=ContentType::parse(&file.content_type)?;
            multi=multi.singlepart(EmailAttachment::new(file.filename.clone()).body(decoded(file)?,mime));
        }
        message.multipart(multi)?
    };
    builder.build().send(result).await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_unicode_safely(){
        let chunks=split_chars("你好世界",3);
        assert_eq!(chunks,vec!["你好世","界"]);
    }
    #[tokio::test]
    async fn smtp_delivers_a_mime_attachment() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read);
            write.write_all(b"220 localhost ESMTP ready\r\n").await.unwrap();
            let mut email = String::new();
            let mut inside_data = false;
            loop {
                let mut line = String::new();
                if lines.read_line(&mut line).await.unwrap() == 0 { break; }
                if inside_data {
                    if line == ".\r\n" || line == ".\n" {
                        write.write_all(b"250 accepted\r\n").await.unwrap();
                        break;
                    }
                    email.push_str(&line);
                    continue;
                }
                let reply: &[u8] = if line.starts_with("EHLO ") || line.starts_with("HELO ") {
                    b"250-localhost\r\n250 8BITMIME\r\n"
                } else if line.starts_with("MAIL FROM:") || line.starts_with("RCPT TO:") {
                    b"250 OK\r\n"
                } else if line.starts_with("DATA") {
                    inside_data = true;
                    b"354 send data\r\n"
                } else {
                    panic!("Unexpected SMTP command: {line:?}");
                };
                write.write_all(reply).await.unwrap();
            }
            email
        });
        let item = RelayItem {
            id: Uuid::new_v4(), kind: "email_smtp".into(),
            settings: json!({
                "host":"127.0.0.1","port":port,"tls":"none",
                "from":"sender@example.com","to":["receiver@example.com"]
            }),
            title:"Integration test".into(), body:"Mail body".into(),
            attachments:vec![Attachment {
                filename:"report.txt".into(), content_type:"text/plain".into(),
                data_base64:"aGVsbG8=".into()
            }]
        };
        let result = tokio::time::timeout(Duration::from_secs(8), send_smtp(&item))
            .await.expect("SMTP delivery timed out");
        assert!(result.is_ok(), "SMTP error: {result:?}");
        let email = tokio::time::timeout(Duration::from_secs(8), server)
            .await.expect("SMTP server timed out").unwrap();
        assert!(email.contains("multipart/mixed"));
        assert!(email.contains("report.txt"));
        assert!(email.contains("hello") || email.contains("aGVsbG8="));
    }

    #[tokio::test]
    async fn postgres_notification_is_delivered_after_commit() {
        let Ok(url) = std::env::var("DATABASE_URL") else { return; };
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let mut listener = PgListener::connect_with(&pool).await.unwrap();
        listener.listen(DELIVERY_CHANNEL).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("NOTIFY messagepusher_delivery")
            .execute(&mut *tx).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(150), listener.recv()).await.is_err());
        tx.commit().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), listener.recv())
            .await.unwrap().unwrap();
    }

    #[test]
    fn proxy_batches_fit_request_limit_without_reordering() {
        let jobs: Vec<Job> = (0..8).map(|i| Job {
            item: RelayItem {
                id: Uuid::from_u128((i + 1) as u128),
                kind: "telegram".into(),
                settings: json!({"bot_token":"123:secret","chat_id":"123"}),
                title: format!("message {i}"),
                body: String::new(),
                attachments: vec![Attachment {
                    filename: format!("report-{i}.txt"),
                    content_type: "text/plain".into(),
                    data_base64: "Y".repeat(1_000_000),
                }],
            },
            attempts: 1,
            delivery: DeliveryMode::Proxy {
                url: "https://relay.example.com".into(),
                secret: "placeholder".into(),
            },
        }).collect();
        let groups = split_proxy_batches(jobs);
        let total: usize = groups.iter().map(Vec::len).sum();
        assert_eq!(total, 8);
        assert!(groups.len() > 1);
        let mut expected = 1u128;
        for group in groups {
            let body = serde_json::to_vec(&RelayBatch {
                items: group.into_iter().map(|job| {
                    assert_eq!(job.item.id, Uuid::from_u128(expected));
                    expected += 1;
                    job.item
                }).collect(),
            }).unwrap();
            assert!(body.len() <= MAX_PROXY_BATCH_BYTES);
        }
    }

    #[test]
    fn exponential_retry_has_an_upper_limit() {
        assert_eq!(retry_delay_seconds(1), 40);
        assert_eq!(retry_delay_seconds(2), 80);
        assert_eq!(retry_delay_seconds(5), 640);
        assert!(retry_delay_seconds(20) <= 1800);
    }

    #[test]
    fn endpoint_hosts_are_fixed(){
        let u=url::Url::parse("https://qyapi.weixin.qq.com/cgi-bin/webhook/send").unwrap();
        assert_eq!(u.host_str(),Some("qyapi.weixin.qq.com"));
    }
}
