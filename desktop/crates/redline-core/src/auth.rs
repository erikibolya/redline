//! Token auth for a port that agents can reach.
//!
//! `v0.1` was "localhost, no auth". A listening port that accepts uploads from other processes
//! needs more than that: any local process, and any web page the browser loads, can reach
//! `127.0.0.1`. So every data route requires a bearer token, the human-facing editor accepts the
//! token once in the URL and trades it for an `HttpOnly` cookie.

use base64::Engine;
use rand::RngCore;

pub const COOKIE_NAME: &str = "redline_token";
pub const SESSION_COOKIE_NAME: &str = "redline_session";
pub const CSRF_COOKIE_NAME: &str = "redline_csrf";
pub const QUERY_PARAM: &str = "t";
pub const SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;

/// 32 random bytes, base64url without padding. The same shape is used for sessions, CSRF
/// tokens, PATs and agent tokens; only the server-side hash is stored for the secret ones.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Clone, Debug)]
pub struct Auth {
    token: String,
}

impl Auth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// 32 random bytes, base64url without padding — same shape as a GitHub token, easy to paste.
    pub fn generate() -> Self {
        Self {
            token: random_token(),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Constant-time comparison: a local attacker should not be able to guess the token
    /// byte-by-byte from response timing.
    pub fn verify(&self, candidate: &str) -> bool {
        let expected = self.token.as_bytes();
        let given = candidate.as_bytes();
        if expected.len() != given.len() {
            return false;
        }
        let mut diff = 0u8;
        for (a, b) in expected.iter().zip(given) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    Bearer,
    SessionCookie,
    LegacyCookie,
    Query,
}

#[derive(Debug, Clone)]
pub struct Credential {
    pub value: String,
    pub source: CredentialSource,
}

/// Pull the first usable credential out of a request: `Authorization: Bearer`, the session
/// cookie, the legacy token cookie, or a `?t=` query parameter. A session cookie outranks the
/// legacy cookie so logging in as a member cannot silently fall back to the owner token.
pub fn credential_detail_from(
    headers: &axum::http::HeaderMap,
    query: Option<&str>,
) -> Option<Credential> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(token) = text.strip_prefix("Bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(Credential {
                        value: token.to_string(),
                        source: CredentialSource::Bearer,
                    });
                }
            }
        }
    }
    if let Some(token) = cookie_value(headers, SESSION_COOKIE_NAME) {
        return Some(Credential {
            value: token,
            source: CredentialSource::SessionCookie,
        });
    }
    if let Some(token) = cookie_value(headers, COOKIE_NAME) {
        return Some(Credential {
            value: token,
            source: CredentialSource::LegacyCookie,
        });
    }
    for (key, value) in query
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
    {
        if key == QUERY_PARAM && !value.is_empty() {
            return Some(Credential {
                value: value.to_string(),
                source: CredentialSource::Query,
            });
        }
    }
    None
}

pub fn credential_from(headers: &axum::http::HeaderMap, query: Option<&str>) -> Option<String> {
    credential_detail_from(headers, query).map(|credential| credential.value)
}

fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(axum::http::header::COOKIE)?;
    let text = value.to_str().ok()?;
    for part in text.split(';') {
        let part = part.trim();
        if let Some(token) = part.strip_prefix(&format!("{name}=")) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

pub fn cookie_header(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000")
}

pub fn session_cookie_header(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECS}{secure}"
    )
}

/// The CSRF cookie is intentionally readable by the page: the UI mirrors it into the
/// `X-CSRF-Token` header. It is not an authentication credential by itself.
pub fn csrf_cookie_header(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{CSRF_COOKIE_NAME}={token}; Path=/; SameSite=Strict; Max-Age={SESSION_TTL_SECS}{secure}"
    )
}

pub fn clear_cookie_header(name: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{name}=; Path=/; SameSite=Strict; Max-Age=0{secure}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_unique_and_url_safe() {
        let a = Auth::generate();
        let b = Auth::generate();
        assert_ne!(a.token(), b.token());
        assert_eq!(a.token().len(), 43);
        assert!(a
            .token()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn verify_is_exact() {
        let auth = Auth::new("secret");
        assert!(auth.verify("secret"));
        assert!(!auth.verify("secre"));
        assert!(!auth.verify("secret "));
        assert!(!auth.verify(""));
    }

    #[test]
    fn credentials_come_from_header_cookie_or_query() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(credential_from(&headers, None).as_deref(), Some("abc"));

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("cookie", "other=1; redline_token=xyz".parse().unwrap());
        assert_eq!(credential_from(&headers, None).as_deref(), Some("xyz"));

        assert_eq!(
            credential_from(&axum::http::HeaderMap::new(), Some("a=1&t=q1")).as_deref(),
            Some("q1")
        );
        assert_eq!(credential_from(&axum::http::HeaderMap::new(), None), None);
    }

    #[test]
    fn a_session_cookie_outranks_the_legacy_token_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "cookie",
            "redline_token=owner; redline_session=member"
                .parse()
                .unwrap(),
        );
        let credential = credential_detail_from(&headers, None).unwrap();
        assert_eq!(credential.value, "member");
        assert_eq!(credential.source, CredentialSource::SessionCookie);
    }

    #[test]
    fn cookie_headers_are_http_only_and_can_be_secure() {
        let session = session_cookie_header("s", true);
        assert!(session.contains("HttpOnly"));
        assert!(session.contains("SameSite=Strict"));
        assert!(session.contains("; Secure"));
        let csrf = csrf_cookie_header("c", false);
        assert!(!csrf.contains("HttpOnly"));
        assert!(!csrf.contains("Secure"));
        assert!(csrf.contains(&format!("{CSRF_COOKIE_NAME}=c")));
    }
}
