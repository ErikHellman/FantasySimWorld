//! Sign-in providers and the identity they vouch for.
//!
//! Both providers run the authorization-code flow with PKCE, `state` and a
//! `nonce` (the `oauth2` crate builds the URLs and exchanges the code), then
//! the id_token is checked here with `jsonwebtoken`: signature by the JWKS key
//! named in `kid` (fetched again when the kid is unknown), `iss`, `aud`, `exp`
//! and `nonce`. `email_verified` may be a boolean or, from Apple, a string.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{Jwk, JwkSet, KeyAlgorithm};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse, BasicTokenType,
};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    ExtraTokenFields, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, StandardRevocableToken,
    StandardTokenResponse, TokenUrl,
};
use serde::{Deserialize, Deserializer, Serialize};

use super::apple::Apple;
use super::google::Google;

/// The providers this server knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Google,
    Apple,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Google, ProviderKind::Apple];

    /// The name in URLs, `AUTH_PROVIDERS` and the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }
}

/// Who a provider says signed in, after the id_token checked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub provider: ProviderKind,
    /// the provider's stable user id (`sub`)
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    /// Apple sends the name only on the first sign-in
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum AuthError {
    /// the provider could not be reached or answered nonsense
    Upstream(String),
    /// the provider refused the code (expired, reused, wrong verifier)
    Exchange(String),
    /// the id_token did not check out
    Token(String),
    Config(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Upstream(why) => write!(f, "provider unavailable: {why}"),
            Self::Exchange(why) => write!(f, "code exchange failed: {why}"),
            Self::Token(why) => write!(f, "id_token rejected: {why}"),
            Self::Config(why) => write!(f, "configuration: {why}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// What the code flow needs per request.
pub struct AuthorizeRequest<'a> {
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub pkce: PkceCodeChallenge,
}

/// A code coming back on the callback.
pub struct Callback<'a> {
    pub redirect_uri: &'a str,
    pub code: &'a str,
    pub pkce_verifier: &'a str,
    pub nonce: &'a str,
    /// Apple's `user` form field (JSON with the name), first sign-in only
    pub apple_user: Option<&'a str>,
}

/// An enabled provider.
pub enum Provider {
    Google(Google),
    Apple(Apple),
}

impl Provider {
    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Google(_) => ProviderKind::Google,
            Self::Apple(_) => ProviderKind::Apple,
        }
    }

    /// Where to send the browser to sign in.
    pub async fn authorize_url(
        &self,
        http: &reqwest::Client,
        request: AuthorizeRequest<'_>,
    ) -> Result<String, AuthError> {
        match self {
            Self::Google(google) => {
                let endpoints = google.endpoints(http).await?;
                let client = client(&google.config.client_id, None, endpoints, request.redirect_uri)?;
                Ok(authorize(&client, &request, &["openid", "email", "profile"], &[]))
            }
            Self::Apple(apple) => {
                let client = client(&apple.config.services_id, None, &apple.endpoints, request.redirect_uri)?;
                // Apple posts the result back when name or email is asked for.
                Ok(authorize(&client, &request, &["name", "email"], &[("response_mode", "form_post")]))
            }
        }
    }

    /// Trades the code for an id_token and checks it.
    pub async fn exchange(&self, http: &reqwest::Client, callback: Callback<'_>) -> Result<Identity, AuthError> {
        let (client_id, secret, endpoints) = match self {
            Self::Google(google) => {
                (&google.config.client_id, google.config.client_secret.clone(), google.endpoints(http).await?)
            }
            Self::Apple(apple) => (&apple.config.services_id, apple.client_secret()?, &apple.endpoints),
        };
        let client = client(client_id, Some(secret), endpoints, callback.redirect_uri)?;
        let id_token = exchange_code(http, &client, callback.code, callback.pkce_verifier).await?;
        let claims = verify_id_token(http, endpoints, client_id, &id_token, callback.nonce).await?;
        let name = match self {
            Self::Google(_) => claims.name,
            Self::Apple(_) => callback.apple_user.and_then(super::apple::name_from_user_field).or(claims.name),
        };
        Ok(Identity {
            provider: self.kind(),
            subject: claims.sub,
            email: claims.email,
            email_verified: claims.email_verified,
            name,
        })
    }
}

/// A provider's URLs and keys.
pub struct OidcEndpoints {
    /// accepted `iss` values
    pub issuers: Vec<String>,
    pub authorize_url: String,
    pub token_url: String,
    pub jwks: Jwks,
}

/// The token endpoint's answer: the standard fields plus `id_token`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IdTokenField {
    pub id_token: String,
}

impl ExtraTokenFields for IdTokenField {}

type OidcTokenResponse = StandardTokenResponse<IdTokenField, BasicTokenType>;

type OidcClient = oauth2::Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

fn client(
    client_id: &str,
    secret: Option<String>,
    endpoints: &OidcEndpoints,
    redirect_uri: &str,
) -> Result<OidcClient, AuthError> {
    let bad = |what: &str, e: oauth2::url::ParseError| AuthError::Config(format!("{what}: {e}"));
    let mut client = oauth2::Client::new(ClientId::new(client_id.to_owned()))
        .set_auth_uri(AuthUrl::new(endpoints.authorize_url.clone()).map_err(|e| bad("authorize url", e))?)
        .set_token_uri(TokenUrl::new(endpoints.token_url.clone()).map_err(|e| bad("token url", e))?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_owned()).map_err(|e| bad("redirect url", e))?)
        // client_secret_post: both providers take it, and Apple's secret is a long JWT.
        .set_auth_type(AuthType::RequestBody);
    if let Some(secret) = secret {
        client = client.set_client_secret(ClientSecret::new(secret));
    }
    Ok(client)
}

fn authorize(client: &OidcClient, request: &AuthorizeRequest<'_>, scopes: &[&str], extra: &[(&str, &str)]) -> String {
    let state = request.state.to_owned();
    let mut builder = client
        .authorize_url(move || CsrfToken::new(state))
        .add_scopes(scopes.iter().map(|s| Scope::new((*s).to_owned())))
        .set_pkce_challenge(request.pkce.clone())
        .add_extra_param("nonce", request.nonce);
    for (name, value) in extra {
        builder = builder.add_extra_param(*name, *value);
    }
    builder.url().0.to_string()
}

/// oauth2's HTTP hook, served by our reqwest client (rustls on aws-lc-rs).
async fn send(http: reqwest::Client, request: oauth2::HttpRequest) -> Result<oauth2::HttpResponse, reqwest::Error> {
    let (parts, body) = request.into_parts();
    let response = http.request(parts.method, parts.uri.to_string()).headers(parts.headers).body(body).send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut out = oauth2::HttpResponse::new(response.bytes().await?.to_vec());
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
}

async fn exchange_code(
    http: &reqwest::Client,
    client: &OidcClient,
    code: &str,
    verifier: &str,
) -> Result<String, AuthError> {
    let call = |request| send(http.clone(), request);
    let token = client
        .exchange_code(AuthorizationCode::new(code.to_owned()))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
        .request_async(&call)
        .await
        .map_err(|e| match e {
            oauth2::RequestTokenError::ServerResponse(r) => AuthError::Exchange(r.to_string()),
            other => AuthError::Upstream(other.to_string()),
        })?;
    Ok(token.extra_fields().id_token.clone())
}

/// A JWKS endpoint with its keys cached until an unknown `kid` shows up.
pub struct Jwks {
    url: String,
    cache: Mutex<(Arc<JwkSet>, Option<Instant>)>,
}

/// Unknown kids refetch the set at most this often (a forged kid must not turn us into a crawler).
const JWKS_REFETCH: Duration = Duration::from_secs(10);

impl Jwks {
    pub fn new(url: String) -> Self {
        Self { url, cache: Mutex::new((Arc::new(JwkSet { keys: Vec::new() }), None)) }
    }

    async fn key(&self, http: &reqwest::Client, kid: &str) -> Result<Jwk, AuthError> {
        let (set, fetched) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(jwk) = set.find(kid) {
            return Ok(jwk.clone());
        }
        if fetched.is_some_and(|at| at.elapsed() < JWKS_REFETCH) {
            return Err(AuthError::Token(format!("unknown signing key {kid:?}")));
        }
        let set: JwkSet = fetch_json(http, &self.url).await?;
        let found = set.find(kid).cloned();
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = (Arc::new(set), Some(Instant::now()));
        found.ok_or_else(|| AuthError::Token(format!("unknown signing key {kid:?}")))
    }
}

pub(crate) async fn fetch_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
) -> Result<T, AuthError> {
    let upstream = |e: reqwest::Error| AuthError::Upstream(format!("{url}: {e}"));
    http.get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(upstream)?
        .json()
        .await
        .map_err(upstream)
}

/// The id_token claims we use.
#[derive(Debug, Deserialize)]
pub struct IdClaims {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub email_verified: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
}

/// `true`, `"true"` (Apple) → true; anything else → false.
fn lenient_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::String(s) => s.eq_ignore_ascii_case("true"),
        _ => false,
    })
}

pub async fn verify_id_token(
    http: &reqwest::Client,
    endpoints: &OidcEndpoints,
    client_id: &str,
    id_token: &str,
    nonce: &str,
) -> Result<IdClaims, AuthError> {
    let bad = |e: jsonwebtoken::errors::Error| AuthError::Token(e.to_string());
    let header = jsonwebtoken::decode_header(id_token).map_err(bad)?;
    if !matches!(header.alg, Algorithm::RS256 | Algorithm::ES256) {
        return Err(AuthError::Token(format!("algorithm {:?} not accepted", header.alg)));
    }
    let kid = header.kid.ok_or_else(|| AuthError::Token("no kid".into()))?;
    let jwk = endpoints.jwks.key(http, &kid).await?;
    let key_alg = match jwk.common.key_algorithm {
        None => None,
        Some(KeyAlgorithm::RS256) => Some(Algorithm::RS256),
        Some(KeyAlgorithm::ES256) => Some(Algorithm::ES256),
        Some(other) => return Err(AuthError::Token(format!("key algorithm {other:?} not accepted"))),
    };
    if key_alg.is_some_and(|alg| alg != header.alg) {
        return Err(AuthError::Token("token and key algorithms differ".into()));
    }
    let key = DecodingKey::from_jwk(&jwk).map_err(bad)?;
    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&endpoints.issuers);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = jsonwebtoken::decode::<IdClaims>(id_token, &key, &validation).map_err(bad)?.claims;
    match &claims.nonce {
        Some(got) if super::same(got.as_bytes(), nonce.as_bytes()) => Ok(claims),
        _ => Err(AuthError::Token("nonce does not match".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_verified_is_lenient() {
        let parse = |json: &str| serde_json::from_str::<IdClaims>(json).unwrap().email_verified;
        assert!(parse(r#"{"sub":"1","email_verified":true}"#));
        assert!(parse(r#"{"sub":"1","email_verified":"true"}"#));
        assert!(!parse(r#"{"sub":"1","email_verified":"false"}"#));
        assert!(!parse(r#"{"sub":"1","email_verified":1}"#));
        assert!(!parse(r#"{"sub":"1"}"#));
    }

    #[test]
    fn provider_names() {
        assert_eq!(ProviderKind::parse("apple"), Some(ProviderKind::Apple));
        assert_eq!(ProviderKind::parse("Google"), None);
    }
}
