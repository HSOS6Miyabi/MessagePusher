use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use argon2::{password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString}, Argon2};
use base64::{engine::general_purpose::{STANDARD}, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct Crypto { key: Arc<[u8; 32]> }

impl Crypto {
    pub fn new(encoded: &str) -> Result<Self> {
        let bytes = STANDARD.decode(encoded).context("CONFIG_KEY_BASE64 must be Base64")?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("CONFIG_KEY_BASE64 must decode to exactly 32 bytes"))?;
        Ok(Self { key: Arc::new(key) })
    }
    pub fn encrypt(&self, plain: &str) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).expect("32-byte key");
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;
        let mut result = nonce.to_vec();
        result.extend(ciphertext);
        Ok(STANDARD.encode(result))
    }
    pub fn decrypt(&self, sealed: &str) -> Result<String> {
        let blob = STANDARD.decode(sealed)?;
        if blob.len() < 28 { bail!("Invalid encrypted record"); }
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).expect("32-byte key");
        let bytes = cipher.decrypt(Nonce::from_slice(&blob[..12]), &blob[12..])
            .map_err(|_| anyhow::anyhow!("Unable to decrypt provider settings"))?;
        Ok(String::from_utf8(bytes)?)
    }
}

pub fn hash_secret(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}
pub fn random_secret(prefix: &str) -> String {
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    format!("{}{}", prefix, hex::encode(bytes))
}
pub fn password_hash(password: &str) -> Result<String> {
    if password.len() < 12 || password.len() > 256 { bail!("Password length must be between 12 and 256 characters"); }
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt).map_err(|e| anyhow::anyhow!(e.to_string()))?.to_string())
}
pub fn password_valid(encoded: &str, password: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| Argon2::default().verify_password(password.as_bytes(), &hash).is_ok())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claims { sub: String, exp: usize, iat: usize }
pub fn session_token(user_id: Uuid, secret: &[u8]) -> Result<String> {
    let issued = Utc::now().timestamp() as usize;
    Ok(encode(&Header::new(Algorithm::HS256), &Claims { sub: user_id.to_string(), iat: issued, exp: issued + 86400 }, &EncodingKey::from_secret(secret))?)
}
pub fn verify_session(token: &str, secret: &[u8]) -> Result<Uuid> {
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret), &Validation::new(Algorithm::HS256))?;
    Ok(Uuid::parse_str(&data.claims.sub)?)
}
pub fn hmac_hex(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut signer = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("valid hmac key");
    signer.update(timestamp.to_string().as_bytes());
    signer.update(b".");
    signer.update(body);
    hex::encode(signer.finalize().into_bytes())
}
pub fn secure_equal(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encryption_roundtrip_and_tamper_detection() {
        let key = STANDARD.encode([42u8;32]);
        let cipher = Crypto::new(&key).unwrap();
        let encrypted = cipher.encrypt(r#"{"token":"secret"}"#).unwrap();
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), r#"{"token":"secret"}"#);
        let mut raw = STANDARD.decode(encrypted).unwrap();
        let n = raw.len()-1;
        raw[n] ^= 1;
        assert!(cipher.decrypt(&STANDARD.encode(raw)).is_err());
    }
    #[test]
    fn password_and_session() {
        let pass = password_hash("a-long-password123").unwrap();
        assert!(password_valid(&pass,"a-long-password123"));
        assert!(!password_valid(&pass,"incorrect-password"));
        let uid = Uuid::new_v4();
        let token = session_token(uid,b"a very long private jwt secret for tests").unwrap();
        assert_eq!(verify_session(&token,b"a very long private jwt secret for tests").unwrap(), uid);
    }
    #[test]
    fn hmac_is_stable() {
        assert_eq!(hmac_hex("secret",12,b"abc"), hmac_hex("secret",12,b"abc"));
        assert_ne!(hmac_hex("secret",12,b"abc"), hmac_hex("secret",12,b"abd"));
    }
}
