//! Google: endpoints from the discovery document, fetched on first use.

use serde::Deserialize;
use tokio::sync::OnceCell;

use super::provider::{AuthError, Jwks, OidcEndpoints, fetch_json};
use crate::config::GoogleConfig;

/// Google's id_tokens carry either spelling.
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

pub struct Google {
    pub config: GoogleConfig,
    endpoints: OnceCell<OidcEndpoints>,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

impl Google {
    pub fn new(config: GoogleConfig) -> Self {
        Self { config, endpoints: OnceCell::new() }
    }

    /// The discovered endpoints; a failed discovery is retried on the next sign-in.
    pub async fn endpoints(&self, http: &reqwest::Client) -> Result<&OidcEndpoints, AuthError> {
        self.endpoints
            .get_or_try_init(|| async {
                let found: Discovery = fetch_json(http, &self.config.discovery_url).await?;
                let mut issuers: Vec<String> = ISSUERS.iter().map(|s| (*s).to_owned()).collect();
                if !issuers.contains(&found.issuer) {
                    issuers.push(found.issuer);
                }
                Ok(OidcEndpoints {
                    issuers,
                    authorize_url: found.authorization_endpoint,
                    token_url: found.token_endpoint,
                    jwks: Jwks::new(found.jwks_uri),
                })
            })
            .await
    }
}
