//! Service-token auth for the registry's write path.
//!
//! Only the compositor posts proofs, so the write path takes a bearer token
//! rather than an end-user identity. Reads stay public: an unlisted proof is
//! already protected by its slug.

use worker_stack::prelude::*;

/// The worker secret the compositor sends as `Authorization: Bearer …`.
pub const PUBLISH_TOKEN_SECRET: &str = "VIEWER_PUBLISH_TOKEN";

/// `Ok(None)` when the request may proceed; `Ok(Some(response))` when it may
/// not, and the handler returns that response unchanged.
pub fn authorize(req: &Request, env: &Env) -> Result<Option<Response>> {
    let expected = match env.secret(PUBLISH_TOKEN_SECRET) {
        Ok(secret) => secret.to_string(),
        // Do not fall open when the secret is missing: an unconfigured worker
        // must refuse writes, not accept them.
        Err(_) => {
            return Ok(Some(Response::error(
                format!("{PUBLISH_TOKEN_SECRET} is not set on this worker"),
                503,
            )?));
        }
    };

    let supplied = req
        .headers()
        .get("Authorization")?
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned));
    let Some(supplied) = supplied else {
        return Ok(Some(Response::error("missing bearer token", 401)?));
    };

    if !constant_time_eq(expected.as_bytes(), supplied.as_bytes()) {
        return Ok(Some(Response::error("invalid bearer token", 401)?));
    }
    Ok(None)
}

/// Compare without an early exit on the first differing byte, so a wrong token
/// gives up no information about how much of it was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn only_an_exact_match_passes() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokeN"));
        assert!(!constant_time_eq(b"token", b"token "));
        assert!(!constant_time_eq(b"", b"token"));
        assert!(constant_time_eq(b"", b""));
    }
}
