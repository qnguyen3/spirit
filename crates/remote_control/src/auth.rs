use base64::Engine as _;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SECRET_BYTES: usize = 32;
const ENCODED_SECRET_LEN: usize = 43;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretParseError {
    #[error("secret must be {expected} characters, got {actual}")]
    Length { expected: usize, actual: usize },
    #[error("secret contains a character outside the base64url alphabet")]
    Alphabet,
}

fn generate_secret() -> String {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn validate_secret(secret: &str) -> Result<(), SecretParseError> {
    if secret.len() != ENCODED_SECRET_LEN {
        return Err(SecretParseError::Length {
            expected: ENCODED_SECRET_LEN,
            actual: secret.len(),
        });
    }
    let alphabet_ok = secret
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if alphabet_ok {
        Ok(())
    } else {
        Err(SecretParseError::Alphabet)
    }
}

fn digest(value: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.finalize().into()
}

pub fn constant_time_bytes_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken(String);

impl AccessToken {
    pub fn generate() -> Self {
        Self(generate_secret())
    }

    pub fn parse(secret: &str) -> Result<Self, SecretParseError> {
        validate_secret(secret)?;
        Ok(Self(secret.to_owned()))
    }

    pub fn reveal(&self) -> &str {
        &self.0
    }

    pub fn masked(&self) -> String {
        "•".repeat(12)
    }

    pub fn constant_time_eq(&self, other: &str) -> bool {
        constant_time_bytes_eq(&digest(&self.0), &digest(other))
    }
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AccessToken(redacted)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    pub fn generate() -> Self {
        Self(generate_secret())
    }

    pub fn parse(secret: &str) -> Result<Self, SecretParseError> {
        validate_secret(secret)?;
        Ok(Self(secret.to_owned()))
    }

    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionId(redacted)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionHash(String);

impl SessionHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn hash_session_id(session_id: &SessionId) -> SessionHash {
    SessionHash(hex_encode(&digest(session_id.reveal())))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(hex_encode(&bytes))
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: DeviceId,
    pub session_hash: SessionHash,
    pub label: String,
    pub created_ts: i64,
    pub last_seen_ts: i64,
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

pub fn device_label_from_user_agent(user_agent: Option<&str>) -> String {
    let Some(user_agent) = user_agent else {
        return "Browser".to_owned();
    };
    let platform = if user_agent.contains("iPhone") {
        Some("iPhone")
    } else if user_agent.contains("iPad") {
        Some("iPad")
    } else if user_agent.contains("Android") {
        Some("Android")
    } else if user_agent.contains("Macintosh") || user_agent.contains("Mac OS X") {
        Some("Mac")
    } else if user_agent.contains("Windows") {
        Some("Windows")
    } else if user_agent.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    let browser = if user_agent.contains("Edg/") {
        Some("Edge")
    } else if user_agent.contains("OPR/") {
        Some("Opera")
    } else if user_agent.contains("Firefox/") {
        Some("Firefox")
    } else if user_agent.contains("Chrome/") || user_agent.contains("CriOS/") {
        Some("Chrome")
    } else if user_agent.contains("Safari/") {
        Some("Safari")
    } else {
        None
    };
    match (platform, browser) {
        (Some(platform), Some(browser)) => format!("{platform} · {browser}"),
        (Some(platform), None) => platform.to_owned(),
        (None, Some(browser)) => browser.to_owned(),
        (None, None) => "Browser".to_owned(),
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
