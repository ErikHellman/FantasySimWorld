//! GitHub: plain OAuth2 (an OAuth App), no id_token. `/user` says who signed in
//! and `/user/emails` which addresses GitHub has verified; the primary address
//! is the one that counts.

use serde::Deserialize;

use super::provider::{AuthError, Identity, ProviderKind, fetch_api};

/// GitHub wants its media type and an API version on every call.
const HEADERS: [(&str, &str); 2] = [("accept", "application/vnd.github+json"), ("x-github-api-version", "2022-11-28")];

#[derive(Deserialize)]
struct User {
    id: u64,
    login: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
pub struct Email {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
}

/// Who signed in, from `api_url` (`https://api.github.com`) with the access token.
pub async fn identity(http: &reqwest::Client, api_url: &str, access_token: &str) -> Result<Identity, AuthError> {
    let user: User = fetch_api(http, &format!("{api_url}/user"), access_token, &HEADERS).await?;
    let emails: Vec<Email> = fetch_api(http, &format!("{api_url}/user/emails"), access_token, &HEADERS).await?;
    let (email, email_verified) = primary_email(&emails);
    Ok(Identity {
        provider: ProviderKind::GitHub,
        subject: user.id.to_string(),
        email,
        email_verified,
        name: user.name.filter(|n| !n.trim().is_empty()).or(Some(user.login)),
    })
}

/// The primary address and whether GitHub has verified it.
pub fn primary_email(emails: &[Email]) -> (Option<String>, bool) {
    emails.iter().find(|e| e.primary).map_or((None, false), |e| (Some(e.email.clone()), e.verified))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(email: &str, primary: bool, verified: bool) -> Email {
        Email { email: email.into(), primary, verified }
    }

    #[test]
    fn only_the_primary_address_counts() {
        let verified = [email("old@example.com", false, true), email("ada@example.com", true, true)];
        assert_eq!(primary_email(&verified), (Some("ada@example.com".into()), true));
        // A verified secondary address does not stand in for an unverified primary one.
        let unverified = [email("old@example.com", false, true), email("ada@example.com", true, false)];
        assert_eq!(primary_email(&unverified), (Some("ada@example.com".into()), false));
        assert_eq!(primary_email(&[]), (None, false));
    }
}
