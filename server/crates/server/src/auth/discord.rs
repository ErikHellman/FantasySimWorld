//! Discord: plain OAuth2, no id_token. `/users/@me` (with the `email` scope)
//! says who signed in, their address and whether Discord has verified it.

use serde::Deserialize;

use super::provider::{AuthError, Identity, ProviderKind, fetch_api};

#[derive(Deserialize)]
struct User {
    id: String,
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    verified: Option<bool>,
}

/// Who signed in, from `api_url` (`https://discord.com/api`) with the access token.
pub async fn identity(http: &reqwest::Client, api_url: &str, access_token: &str) -> Result<Identity, AuthError> {
    let user: User = fetch_api(http, &format!("{api_url}/users/@me"), access_token, &[]).await?;
    Ok(Identity {
        provider: ProviderKind::Discord,
        subject: user.id,
        email: user.email,
        email_verified: user.verified == Some(true),
        name: user.global_name.filter(|n| !n.trim().is_empty()).or(Some(user.username)),
    })
}
