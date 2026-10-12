//! `identify(token)` verification (PRD §4, ADR 0019). HS256 is pinned.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

#[derive(Deserialize)]
struct Claims {
    sub: String,
    #[allow(dead_code)]
    exp: u64,
}

/// Returns the opaque `sub` when signature and expiry are valid.
/// Invalid or missing tokens yield `None`: the event is stored anonymous.
pub fn verify(token: &str, secret: &str) -> Option<String> {
    if token.is_empty() || secret.is_empty() {
        return None;
    }
    let mut v = Validation::new(Algorithm::HS256);
    v.algorithms = vec![Algorithm::HS256];
    v.leeway = 0;
    v.set_required_spec_claims(&["exp", "sub"]);
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &v).ok()?;
    let sub = data.claims.sub;
    if sub.is_empty() || sub.len() > 200 {
        None
    } else {
        Some(sub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::json;

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn tok(alg: Algorithm, secret: &str, sub: &str, exp: u64) -> String {
        encode(
            &Header::new(alg),
            &json!({"sub": sub, "exp": exp}),
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn valid_token_yields_sub() {
        assert_eq!(
            verify(&tok(Algorithm::HS256, "k", "u_1", now() + 300), "k").as_deref(),
            Some("u_1")
        );
    }

    #[test]
    fn expired_wrong_secret_wrong_alg_and_garbage_are_anonymous() {
        assert!(verify(&tok(Algorithm::HS256, "k", "u", now() - 10), "k").is_none());
        assert!(verify(&tok(Algorithm::HS256, "k", "u", now() + 300), "other").is_none());
        // Algorithm pinning: HS384 signed with the right secret is refused.
        assert!(verify(&tok(Algorithm::HS384, "k", "u", now() + 300), "k").is_none());
        assert!(verify("not.a.jwt", "k").is_none());
        assert!(verify("", "k").is_none());
        assert!(verify(&tok(Algorithm::HS256, "k", "u", now() + 300), "").is_none());
    }

    #[test]
    fn alg_none_is_refused() {
        // header {"alg":"none"} . {"sub":"u","exp":9999999999} .
        let t = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ1IiwiZXhwIjo5OTk5OTk5OTk5fQ.";
        assert!(verify(t, "k").is_none());
    }
}
