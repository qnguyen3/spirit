use super::{
    AccessToken, DeviceId, SecretParseError, SessionId, constant_time_bytes_eq,
    device_label_from_user_agent, hash_session_id,
};

#[test]
fn generated_token_uses_the_base64url_alphabet_at_a_fixed_length() {
    let token = AccessToken::generate();
    assert_eq!(token.reveal().len(), 43);
    assert!(
        token
            .reveal()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
}

#[test]
fn generated_tokens_differ() {
    assert_ne!(
        AccessToken::generate().reveal(),
        AccessToken::generate().reveal()
    );
}

#[test]
fn parse_rejects_wrong_length() {
    assert_eq!(
        AccessToken::parse("short"),
        Err(SecretParseError::Length {
            expected: 43,
            actual: 5
        })
    );
}

#[test]
fn parse_rejects_characters_outside_the_alphabet() {
    let bad = "*".repeat(43);
    assert_eq!(AccessToken::parse(&bad), Err(SecretParseError::Alphabet));
}

#[test]
fn parse_accepts_a_generated_token() {
    let token = AccessToken::generate();
    assert_eq!(AccessToken::parse(token.reveal()), Ok(token));
}

#[test]
fn constant_time_eq_matches_only_the_same_secret() {
    let token = AccessToken::generate();
    assert!(token.constant_time_eq(token.reveal()));
    assert!(!token.constant_time_eq(AccessToken::generate().reveal()));
    assert!(!token.constant_time_eq(""));
}

#[test]
fn constant_time_bytes_eq_compares_contents_and_length() {
    assert!(constant_time_bytes_eq(b"abc", b"abc"));
    assert!(!constant_time_bytes_eq(b"abc", b"abd"));
    assert!(!constant_time_bytes_eq(b"abc", b"ab"));
}

#[test]
fn session_hash_is_stable_and_unique() {
    let session = SessionId::generate();
    assert_eq!(hash_session_id(&session), hash_session_id(&session));
    assert_ne!(
        hash_session_id(&session),
        hash_session_id(&SessionId::generate())
    );
    assert_eq!(hash_session_id(&session).as_str().len(), 64);
}

#[test]
fn device_ids_are_hex_and_unique() {
    let id = DeviceId::generate();
    assert_eq!(id.as_str().len(), 16);
    assert!(id.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(DeviceId::generate(), DeviceId::generate());
}

#[test]
fn token_debug_never_reveals_the_secret() {
    let token = AccessToken::generate();
    let rendered = format!("{token:?}");
    assert!(!rendered.contains(token.reveal()));
}

#[test]
fn device_labels_summarize_common_user_agents() {
    assert_eq!(
        device_label_from_user_agent(Some(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Safari/604.1"
        )),
        "iPhone · Safari"
    );
    assert_eq!(
        device_label_from_user_agent(Some(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/120.0 Safari/537.36"
        )),
        "Mac · Chrome"
    );
    assert_eq!(device_label_from_user_agent(None), "Browser");
    assert_eq!(device_label_from_user_agent(Some("curl/8.4.0")), "Browser");
}
