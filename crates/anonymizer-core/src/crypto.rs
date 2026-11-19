//! 可逆加密：AES-256-GCM（口令派生密钥）+ BASE64 编解码。
//! 全部依赖 RustCrypto 系（MIT/Apache-2.0）。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::{RngCore, rngs::OsRng};
use sha2::Sha256;
use std::fmt;

#[derive(Debug)]
pub struct CryptoError(pub String);

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CryptoError {}

/// PBKDF2-SHA256 从口令派生 32 字节密钥
pub fn derive_key(passphrase: &str, salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), salt, rounds, &mut key).expect("PBKDF2 不应失败");
    key
}

/// 生成随机 IV 并 AES-256-GCM 加密，输出 `AES:{b64(iv)}:{b64(ciphertext)}`
pub fn aes_encrypt(plain: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut iv);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| CryptoError(e.to_string()))?;
    let nonce = Nonce::from_slice(&iv);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| CryptoError(format!("加密失败: {e}")))?;
    Ok(format!("AES:{}:{}", B64.encode(iv), B64.encode(ct)))
}

/// 解密 `AES:{iv}:{ct}`
pub fn aes_decrypt(payload: &str, key: &[u8; 32]) -> Result<String, CryptoError> {
    let parts: Vec<&str> = payload.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != "AES" {
        return Err(CryptoError("格式错误：应为 AES:{iv}:{ct}".into()));
    }
    let iv = B64
        .decode(parts[1])
        .map_err(|e| CryptoError(format!("IV 解码失败: {e}")))?;
    let ct = B64
        .decode(parts[2])
        .map_err(|e| CryptoError(format!("密文解码失败: {e}")))?;
    // IV 必须恰好 12 字节：数据被篡改/截断时 Nonce::from_slice 会 panic，必须先校验并返回错误
    if iv.len() != 12 {
        return Err(CryptoError("密文已损坏（IV 长度不正确）".into()));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| CryptoError(e.to_string()))?;
    let nonce = Nonce::from_slice(&iv);
    let plain = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|_| CryptoError("解密失败（口令错误或数据损坏）".to_string()))?;
    String::from_utf8(plain).map_err(|e| CryptoError(format!("解码失败: {e}")))
}

pub fn base64_encode(s: &str) -> String {
    B64.encode(s.as_bytes())
}

pub fn base64_decode(s: &str) -> Result<String, CryptoError> {
    let bytes = B64
        .decode(s)
        .map_err(|e| CryptoError(format!("BASE64 解码失败: {e}")))?;
    String::from_utf8(bytes).map_err(|e| CryptoError(format!("解码失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_roundtrip() {
        let key = derive_key("s3cret", b"fixed-salt", 1_000);
        let ct = aes_encrypt("13800138000", &key).unwrap();
        assert!(ct.starts_with("AES:"));
        let plain = aes_decrypt(&ct, &key).unwrap();
        assert_eq!(plain, "13800138000");
    }

    #[test]
    fn aes_wrong_key_fails() {
        let k1 = derive_key("a", b"salt", 1_000);
        let k2 = derive_key("b", b"salt", 1_000);
        let ct = aes_encrypt("机密", &k1).unwrap();
        assert!(aes_decrypt(&ct, &k2).is_err());
    }

    #[test]
    fn base64_roundtrip() {
        let enc = base64_encode("北京降本增笑有限公司");
        assert_eq!(base64_decode(&enc).unwrap(), "北京降本增笑有限公司");
    }

    #[test]
    fn decrypt_rejects_malformed_iv_without_panic() {
        let key = derive_key("pw", b"salt", 1000);
        let good = aes_encrypt("hello", &key).unwrap();
        // 篡改 IV 长度
        let bad = good.replacen("AES:", "AES:AAAA", 1);
        assert!(aes_decrypt(&bad, &key).is_err());
        assert!(aes_decrypt("AES::", &key).is_err());
        assert!(aes_decrypt("AES:AAAA:BBBB", &key).is_err());
        assert_eq!(aes_decrypt(&good, &key).unwrap(), "hello");
    }
}
