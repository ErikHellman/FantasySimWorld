//! Sign in with Apple.
//!
//! The Services ID is the client id; the client secret is a JWT signed ES256
//! with the `.p8` key, minted for 30 days and minted again when less than a
//! day remains (Apple accepts up to six months). The name arrives only on the
//! first sign-in, in the `user` form field of the `form_post` callback.

use std::sync::Mutex;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use super::provider::{AuthError, Jwks, OidcEndpoints};
use crate::config::AppleConfig;

/// The `aud` Apple wants in client secrets.
const APPLE_AUDIENCE: &str = "https://appleid.apple.com";
const DAY: u64 = 24 * 3600;
pub const SECRET_LIFETIME: u64 = 30 * DAY;
/// Mint a new secret when the cached one has less than this left.
pub const SECRET_REMINT: u64 = DAY;

pub struct Apple {
    pub config: AppleConfig,
    pub endpoints: OidcEndpoints,
    key: EncodingKey,
    /// the current client secret and its `exp`
    secret: Mutex<Option<(String, u64)>>,
}

#[derive(Serialize)]
struct SecretClaims<'a> {
    iss: &'a str,
    iat: u64,
    exp: u64,
    aud: &'a str,
    sub: &'a str,
}

impl Apple {
    pub fn new(config: AppleConfig) -> Result<Self, AuthError> {
        let key = EncodingKey::from_ec_pem(config.private_key_pem.as_bytes())
            .map_err(|e| AuthError::Config(format!("APPLE_PRIVATE_KEY: {e}")))?;
        let endpoints = OidcEndpoints {
            issuers: vec![config.endpoints.issuer.clone()],
            authorize_url: config.endpoints.authorize_url.clone(),
            token_url: config.endpoints.token_url.clone(),
            jwks: Jwks::new(config.endpoints.jwks_url.clone()),
        };
        Ok(Self { config, endpoints, key, secret: Mutex::new(None) })
    }

    /// The client secret for a token request, minted again when about to expire.
    pub fn client_secret(&self) -> Result<String, AuthError> {
        self.client_secret_at(jsonwebtoken::get_current_timestamp())
    }

    pub fn client_secret_at(&self, now: u64) -> Result<String, AuthError> {
        let mut cached = self.secret.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((jwt, exp)) = cached.as_ref()
            && !needs_remint(*exp, now)
        {
            return Ok(jwt.clone());
        }
        let exp = now + SECRET_LIFETIME;
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.config.key_id.clone());
        let claims = SecretClaims {
            iss: &self.config.team_id,
            iat: now,
            exp,
            aud: APPLE_AUDIENCE,
            sub: &self.config.services_id,
        };
        let jwt = jsonwebtoken::encode(&header, &claims, &self.key)
            .map_err(|e| AuthError::Config(format!("signing the Apple client secret: {e}")))?;
        *cached = Some((jwt.clone(), exp));
        Ok(jwt)
    }
}

/// Pure: whether a secret expiring at `exp` should be replaced at `now`.
pub fn needs_remint(exp: u64, now: u64) -> bool {
    exp.saturating_sub(now) < SECRET_REMINT
}

#[derive(Deserialize)]
struct UserField {
    #[serde(default)]
    name: Option<NameField>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NameField {
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
}

/// "Ada Lovelace" from `{"name":{"firstName":"Ada","lastName":"Lovelace"},"email":"…"}`.
pub fn name_from_user_field(json: &str) -> Option<String> {
    let name = serde_json::from_str::<UserField>(json).ok()?.name?;
    let full = [name.first_name, name.last_name].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let full = full.trim();
    (!full.is_empty()).then(|| full.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remint_when_under_a_day_is_left() {
        assert!(!needs_remint(1000 + SECRET_LIFETIME, 1000));
        assert!(!needs_remint(1000 + DAY, 1000));
        assert!(needs_remint(1000 + DAY - 1, 1000));
        assert!(needs_remint(10, 1000));
    }

    #[test]
    fn the_secret_is_cached_until_a_day_is_left() {
        use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
        use base64::Engine as _;
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &aws_lc_rs::rand::SystemRandom::new())
            .unwrap();
        let body = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let apple = Apple::new(AppleConfig {
            team_id: "TEAM".into(),
            key_id: "KEY".into(),
            services_id: "com.example.web".into(),
            private_key_pem: format!("-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n"),
            endpoints: Default::default(),
        })
        .unwrap();
        let now = 1_800_000_000;
        let first = apple.client_secret_at(now).unwrap();
        assert_eq!(apple.client_secret_at(now + SECRET_LIFETIME - DAY).unwrap(), first);
        let next = apple.client_secret_at(now + SECRET_LIFETIME - DAY + 1).unwrap();
        assert_ne!(next, first);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(next.split('.').nth(1).unwrap()).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(claims["exp"].as_u64().unwrap(), now + 2 * SECRET_LIFETIME - DAY + 1);
        assert_eq!((claims["iss"].as_str(), claims["sub"].as_str()), (Some("TEAM"), Some("com.example.web")));
    }

    #[test]
    fn names_from_the_first_sign_in() {
        let json = r#"{"name":{"firstName":"Ada","lastName":"Lovelace"},"email":"ada@example.com"}"#;
        assert_eq!(name_from_user_field(json).as_deref(), Some("Ada Lovelace"));
        assert_eq!(name_from_user_field(r#"{"name":{"firstName":"Ada"}}"#).as_deref(), Some("Ada"));
        assert_eq!(name_from_user_field(r#"{"email":"a@b"}"#), None);
        assert_eq!(name_from_user_field("not json"), None);
    }
}
