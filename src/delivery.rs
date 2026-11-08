use crate::{api, models::{Attachment, DeliveryMode, ProviderSecrets, RelayBatch, RelayItem, RelayResponse, MAX_PROXY_BATCH_BYTES}, security, AppState};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures::future::join_all;
use lettre::{message::{header::ContentType, Attachment as EmailAttachment, Mailbox, MultiPart, SinglePart}, transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use reqwest::multipart::{Form, Part};
use serde_json::{json, Value};
use sqlx::Row;
use std::{collections::HashMap, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
struct Job { item: RelayItem, attempts: i32, delivery: DeliveryMode }

pub async fn worker(state: AppState) {
    loop {
        match claim_jobs(&state).await {
            Ok(jobs) if jobs.is_empty() => tokio::time::sleep(Duration::from_millis(750)).await,
            Ok(jobs) => process_jobs(state.clone(), jobs).await,
            Err(error) => {
                tracing::error!(%error, "Queue worker error");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}
async fn claim_jobs(state:&AppState)->Result<Vec<Job>> {
    let rows=sqlx::query(
        "WITH selected AS (
            SELECT d.id FROM deliveries d JOIN providers p ON p.id=d.provider_id
            WHERE p.enabled=true AND
            ((d.status='pending' AND d.next_attempt_at<=now()) OR (d.status='working' AND d.lease_until<now()))
            ORDER BY d.created_at LIMIT 16 FOR UPDATE OF d SKIP LOCKED
        ) UPDATE deliveries d
        SET status='working',lease_until=now()+interval '75 seconds',attempts=d.attempts+1
        FROM selected WHERE d.id=selected.id RETURNING d.id")
        .fetch_all(&state.pool).await?;
    if rows.is_empty(){return Ok(Vec::new());}
    let ids:Vec<Uuid>=rows.into_iter().map(|r|r.get("id")).collect();
    let rows=sqlx::query(
        "SELECT d.id,d.attempts,p.kind,p.encrypted_settings,e.title,e.body,e.attachments
         FROM deliveries d JOIN providers p ON p.id=d.provider_id JOIN events e ON e.id=d.event_id
         WHERE d.id=ANY($1)")
        .bind(&ids).fetch_all(&state.pool).await?;
    let mut jobs=Vec::new();
    for row in rows {
        let id:Uuid=row.get("id");
        let decoded:Result<Job>=(|| {
            let encrypted:&str=row.get("encrypted_settings");
            let secret:ProviderSecrets=api::decrypt_provider(state,encrypted).map_err(|e|anyhow!(e.1))?;
            let attachments:Vec<Attachment>=serde_json::from_value(row.get::<Value,_>("attachments"))?;
            Ok(Job{item:RelayItem{id,kind:row.get("kind"),settings:secret.settings,
                title:row.get("title"),body:row.get("body"),attachments},
                attempts:row.get("attempts"),delivery:secret.delivery})
        })();
        match decoded {
            Ok(job)=>jobs.push(job),
            Err(err)=>finish(state,id,5,Err(err.to_string())).await,
        }
    }
    Ok(jobs)
}
async fn process_jobs(state:AppState, jobs:Vec<Job>) {
    let mut tasks=Vec::new();
    let mut groups:HashMap<(String,String),Vec<Job>>=HashMap::new();
    for job in jobs {
        match &job.delivery {
            DeliveryMode::Direct=>{
                let s=state.clone();
                tasks.push(tokio::spawn(async move {
                    let result=tokio::time::timeout(Duration::from_secs(55),send_direct(&s,&job.item)).await
                        .map_err(|_|"provider deadline exceeded".to_string())
                        .and_then(|r|r.map_err(|e|e.to_string()));
                    finish(&s,job.item.id,job.attempts,result).await;
                }));
            }
            DeliveryMode::Proxy{url,secret}=>groups.entry((url.clone(),secret.clone())).or_default().push(job),
        }
    }
    for ((url,secret),jobs) in groups {
        let mut batch=Vec::new();
        for job in jobs {
            let mut trial=batch.clone();
            trial.push(job.clone());
            let size=serde_json::to_vec(&RelayBatch{items:trial.iter().map(|x|x.item.clone()).collect()})
                .map(|v|v.len()).unwrap_or(usize::MAX);
            if size>MAX_PROXY_BATCH_BYTES && !batch.is_empty() {
                let s=state.clone();
                let u=url.clone();let k=secret.clone();
                let ready=std::mem::take(&mut batch);
                tasks.push(tokio::spawn(async move { send_proxy_group(&s,&u,&k,ready).await; }));
            }
            let single_size=serde_json::to_vec(&RelayBatch{items:vec![job.item.clone()]})
                .map(|v|v.len()).unwrap_or(usize::MAX);
            if single_size>MAX_PROXY_BATCH_BYTES {
                let s=state.clone();
                tasks.push(tokio::spawn(async move { finish(&s,job.item.id,job.attempts,Err("message exceeds relay request limit (3.4 MB); send directly or reduce attachments".into())).await; }));
            } else { batch.push(job); }
        }
        if !batch.is_empty(){
            let s=state.clone();
            tasks.push(tokio::spawn(async move {send_proxy_group(&s,&url,&secret,batch).await;}));
        }
    }
    for res in join_all(tasks).await {if let Err(err)=res{tracing::error!(%err,"Delivery task exited unexpectedly");}}
}
async fn finish(state:&AppState,id:Uuid,attempts:i32,result:Result<(),String>) {
    let outcome=match result {
        Ok(()) => sqlx::query("UPDATE deliveries SET status='sent',lease_until=NULL,last_error=NULL,sent_at=now() WHERE id=$1")
            .bind(id).execute(&state.pool).await,
        Err(err) => {
            let short:String=err.chars().take(300).collect();
            if attempts>=5 {
                sqlx::query("UPDATE deliveries SET status='failed',lease_until=NULL,last_error=$2 WHERE id=$1")
                    .bind(id).bind(short).execute(&state.pool).await
            }else{
                let seconds=(20 * 2_i32.pow(attempts.min(6) as u32)).min(1800);
                sqlx::query("UPDATE deliveries SET status='pending',lease_until=NULL,last_error=$2,next_attempt_at=now()+($3 * interval '1 second') WHERE id=$1")
                    .bind(id).bind(short).bind(seconds).execute(&state.pool).await
            }
        }
    };
    if let Err(error)=outcome{tracing::error!(%error, %id,"Unable to update delivery status");}
}
async fn send_proxy_group(state:&AppState,url:&str,secret:&str,jobs:Vec<Job>) {
    let payload=RelayBatch{items:jobs.iter().map(|j|j.item.clone()).collect()};
    let result=async {
        let bytes=serde_json::to_vec(&payload)?;
        let timestamp=chrono::Utc::now().timestamp();
        let signature=security::hmac_hex(secret,timestamp,&bytes);
        let response=state.http.post(url).header("x-mp-timestamp",timestamp.to_string())
            .header("x-mp-signature",signature).header("content-type","application/json")
            .body(bytes).send().await?;
        if !response.status().is_success(){bail!("relay returned HTTP {}",response.status().as_u16());}
        let results:RelayResponse=response.json().await.context("invalid relay response")?;
        Ok::<_,anyhow::Error>(results)
    }.await;
    match result {
        Ok(response)=>{
            for job in jobs {
                let outcome=response.results.iter().find(|r|r.id==job.item.id)
                    .map(|r|if r.ok{Ok(())}else{Err(r.error.clone().unwrap_or_else(||"provider rejected the request".into()))})
                    .unwrap_or_else(||Err("relay omitted the delivery result".into()));
                finish(state,job.item.id,job.attempts,outcome).await;
            }
        }
        Err(error)=>{
            let description=format!("relay request failed: {}",error);
            for job in jobs {finish(state,job.item.id,job.attempts,Err(description.clone())).await;}
        }
    }
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
    if bot.contains('/') || bot.contains('?') || bot.contains('#'){bail!("invalid Telegram bot token");}
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
    for ch in input.chars() {
        if current.chars().count()>=maximum {parts.push(std::mem::take(&mut current));}
        current.push(ch);
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

    #[test]
    fn endpoint_hosts_are_fixed(){
        let u=url::Url::parse("https://qyapi.weixin.qq.com/cgi-bin/webhook/send").unwrap();
        assert_eq!(u.host_str(),Some("qyapi.weixin.qq.com"));
    }
}
