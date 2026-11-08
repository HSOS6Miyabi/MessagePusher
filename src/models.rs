use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::error::{ApiError, ApiResult};

pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PROXY_BATCH_BYTES: usize = 3_400_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    #[serde(default = "default_mime")]
    pub content_type: String,
    pub data_base64: String,
}
fn default_mime() -> String { "application/octet-stream".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest {
    #[serde(default)]
    pub title: String,
    #[serde(default, alias="desp", alias="message", alias="content")]
    pub body: String,
    pub provider_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl SendRequest {
    pub fn validate(&self) -> ApiResult<()> {
        if self.title.trim().is_empty() && self.body.trim().is_empty() { return Err(ApiError::bad("title or body is required")); }
        if self.title.chars().count() > 250 { return Err(ApiError::bad("title is too long (max 250 characters)")); }
        if self.body.len() > 100_000 { return Err(ApiError::bad("body is too long (max 100000 bytes)")); }
        if self.attachments.len() > 5 { return Err(ApiError::bad("at most 5 attachments are allowed")); }
        let mut total = 0;
        for file in &self.attachments {
            if file.filename.trim().is_empty() || file.filename.len() > 120
                || file.filename.contains('/') || file.filename.contains('\\')
                || file.filename.chars().any(char::is_control) || file.filename == "." || file.filename == ".." {
                return Err(ApiError::bad("attachment filename is invalid"));
            }
            if file.content_type.len() > 120 || file.content_type.contains('\n') || file.content_type.contains('\r') {
                return Err(ApiError::bad("invalid attachment content type"));
            }
            let bytes = STANDARD.decode(&file.data_base64).map_err(|_| ApiError::bad("attachment must contain valid Base64"))?;
            total += bytes.len();
            if bytes.is_empty() || total > MAX_TOTAL_ATTACHMENT_BYTES {
                return Err(ApiError::bad("attachments must total 1 byte to 2 MiB"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="mode", rename_all="snake_case")]
pub enum DeliveryMode {
    Direct,
    Proxy { url: String, secret: String },
}
impl Default for DeliveryMode { fn default() -> Self { Self::Direct } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSecrets {
    pub settings: Value,
    #[serde(default)]
    pub delivery: DeliveryMode,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub kind: String,
    pub settings: Value,
    #[serde(default)]
    pub delivery: DeliveryMode,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_true() -> bool { true }

impl ProviderInput {
    pub fn validate(&self) -> ApiResult<()> {
        if self.name.trim().is_empty() || self.name.len() > 100 {
            return Err(ApiError::bad("provider name must be between 1 and 100 bytes"));
        }
        let required: &[&str] = match self.kind.as_str() {
            "telegram" => &["bot_token"],
            "wecom_app" => &["corp_id", "corp_secret", "agent_id"],
            "wecom_webhook" => &["webhook_key"],
            "email_smtp" => &["host", "from", "to"],
            "email_resend" => &["api_key", "from", "to"],
            _ => return Err(ApiError::bad("unknown provider kind")),
        };
        for k in required {
            let exists = self.settings.get(*k).is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()) || v.as_i64().is_some() || v.as_array().is_some_and(|a| !a.is_empty()));
            if !exists { return Err(ApiError::bad(format!("missing provider setting: {}",k))); }
        }
        if let DeliveryMode::Proxy { url, secret } = &self.delivery {
            if self.kind == "email_smtp" { return Err(ApiError::bad("SMTP is not available in the FaaS relay; use email_resend for proxied email")); }
            let parsed = url::Url::parse(url).map_err(|_| ApiError::bad("invalid relay URL"))?;
            if parsed.scheme() != "https" && !(std::env::var("ALLOW_HTTP_RELAY").ok().as_deref() == Some("1") && parsed.scheme() == "http") {
                return Err(ApiError::bad("relay URL must use HTTPS"));
            }
            if parsed.host_str().is_none() || parsed.username() != "" || parsed.password().is_some() || parsed.fragment().is_some() {
                return Err(ApiError::bad("invalid relay URL"));
            }
            if secret.len() < 24 { return Err(ApiError::bad("relay secret must be at least 24 characters")); }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayItem {
    pub id: Uuid,
    pub kind: String,
    pub settings: Value,
    pub title: String,
    pub body: String,
    pub attachments: Vec<Attachment>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayBatch { pub items: Vec<RelayItem> }
#[derive(Debug, Clone, Deserialize)]
pub struct RelayResult { pub id: Uuid, pub ok: bool, pub error: Option<String> }
#[derive(Debug, Clone, Deserialize)]
pub struct RelayResponse { pub results: Vec<RelayResult> }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_security() {
        let mut s = SendRequest{title:"hello".into(),body:String::new(),provider_ids:None,attachments:vec![]};
        assert!(s.validate().is_ok());
        s.attachments.push(Attachment{filename:"../secret".into(),content_type:"text/plain".into(),data_base64:"YQ==".into()});
        assert!(s.validate().is_err());
        s.attachments[0].filename="report.txt".into();
        assert!(s.validate().is_ok());
    }
    #[test]
    fn reject_insecure_relay() {
        let p = ProviderInput{name:"Test".into(),kind:"telegram".into(),
            settings:serde_json::json!({"bot_token":"abc"}),
            delivery:DeliveryMode::Proxy{url:"http://example.com".into(),secret:"a".repeat(32)},enabled:true};
        assert!(p.validate().is_err());
    }
}
