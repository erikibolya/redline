//! Token auth for a port that agents can reach.
//!
//! `v0.1` was "localhost, no auth". A listening port that accepts uploads from other processes
//! needs more than that: any local process, and any web page the browser loads, can reach
//! `127.0.0.1`. So every data route requires a bearer token, the human-facing editor accepts the
//! token once in the URL and trades it for an `HttpOnly` cookie.

use base64::Engine;
use rand::RngCore;

pub const COOKIE_NAME: &str = "redline_token";
pub const QUERY_PARAM: &str = "t";

#[derive(Clone, Debug)]
pub struct Auth {
    token: String,
}

impl Auth {
    pub fn new(token: impl Into<String>) -> Self {
        Self { token: token.into() }
    }

    /// 32 random bytes, base64url without padding — same shape as a GitHub token, easy to paste.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Self {
            token: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
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

/// Pull the first usable credential out of a request: `Authorization: Bearer`, the cookie set by
/// the editor redirect, or a `?t=` query parameter (first contact from a copied review URL).
pub fn credential_from(headers: &axum::http::HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(token) = text.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }
    }
    if let Some(value) = headers.get(axum::http::header::COOKIE) {
        if let Ok(text) = value.to_str() {
            for part in text.split(';') {
                let part = part.trim();
                if let Some(token) = part.strip_prefix(&format!("{COOKIE_NAME}=")) {
                    if !token.is_empty() {
                        return Some(token.to_string());
                    }
                }
            }
        }
    }
    for (key, value) in query.unwrap_or_default().split('&').filter_map(|pair| pair.split_once('=')) {
        if key == QUERY_PARAM && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub fn cookie_header(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000")
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
        assert!(a.token().chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
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
}
