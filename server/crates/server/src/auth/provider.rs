//! Sign-in providers and the identity they vouch for.
//!
//! Every provider runs the authorization-code flow with `state` and, where the
//! provider takes it (all but LinkedIn), PKCE; the `oauth2` crate builds the
//! URLs and exchanges the code. The OpenID Connect providers (Google, Apple,
//! Microsoft, LinkedIn) also get a `nonce`, and their id_token is checked here
//! with `jsonwebtoken`: signature by the JWKS key named in `kid` (fetched again
//! when the kid is unknown), `iss`, `aud`, `exp` and `nonce`. `email_verified`
//! may be a boolean or, from Apple, a string. GitHub and Discord have no
//! id_token: their profile API, called with the access token, says who signed
//! in and whether the e-mail is verified.

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
    StandardTokenResponse, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Deserializer, Serialize};

use super::apple::Apple;
use super::discovered::Discovered;
use super::{discord, github, microsoft};
use crate::config::ApiConfig;

/// The providers this server knows; the wire type, so the login page uses the same names.
pub use crate::api::wire::SignInProvider as ProviderKind;

impl ProviderKind {
    pub const ALL: [ProviderKind; 6] = [
        ProviderKind::Google,
        ProviderKind::Apple,
        ProviderKind::Microsoft,
        ProviderKind::LinkedIn,
        ProviderKind::GitHub,
        ProviderKind::Discord,
    ];

    /// The name in URLs, `AUTH_PROVIDERS` and the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
            Self::Microsoft => "microsoft",
            Self::LinkedIn => "linkedin",
            Self::GitHub => "github",
            Self::Discord => "discord",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }

    /// LinkedIn takes PKCE from native apps only and may refuse it from a web app.
    fn pkce(self) -> bool {
        self != Self::LinkedIn
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
    /// Google, Microsoft or LinkedIn
    Oidc(Discovered),
    Apple(Apple),
    GitHub(ApiConfig),
    Discord(ApiConfig),
}

impl Provider {
    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Oidc(oidc) => oidc.kind,
            Self::Apple(_) => ProviderKind::Apple,
            Self::GitHub(_) => ProviderKind::GitHub,
            Self::Discord(_) => ProviderKind::Discord,
        }
    }

    /// Where to send the browser to sign in.
    pub async fn authorize_url(
        &self,
        http: &reqwest::Client,
        request: AuthorizeRequest<'_>,
    ) -> Result<String, AuthError> {
        let pkce = self.kind().pkce();
        match self {
            Self::Oidc(oidc) => {
                let endpoints = oidc.endpoints(http).await?;
                let client = client(&oidc.config.client_id, None, endpoints.urls(), request.redirect_uri)?;
                Ok(authorize(&client, &request, &["openid", "email", "profile"], pkce, true, &[]))
            }
            Self::Apple(apple) => {
                let client = client(&apple.config.services_id, None, apple.endpoints.urls(), request.redirect_uri)?;
                // Apple posts the result back when name or email is asked for.
                Ok(authorize(&client, &request, &["name", "email"], pkce, true, &[("response_mode", "form_post")]))
            }
            Self::GitHub(api) | Self::Discord(api) => {
                let client = client(&api.client_id, None, api_urls(api), request.redirect_uri)?;
                let scopes: &[&str] =
                    if let Self::GitHub(_) = self { &["read:user", "user:email"] } else { &["identify", "email"] };
                Ok(authorize(&client, &request, scopes, pkce, false, &[]))
            }
        }
    }

    /// Trades the code for tokens and finds out who signed in.
    pub async fn exchange(&self, http: &reqwest::Client, callback: Callback<'_>) -> Result<Identity, AuthError> {
        let kind = self.kind();
        let verifier = kind.pkce().then_some(callback.pkce_verifier);
        match self {
            Self::Oidc(oidc) => {
                let endpoints = oidc.endpoints(http).await?;
                let secret = Some(oidc.config.client_secret.clone());
                let client = client(&oidc.config.client_id, secret, endpoints.urls(), callback.redirect_uri)?;
                let tokens = exchange_code(http, &client, callback.code, verifier).await?;
                // LinkedIn does not promise to send the nonce back; the code itself came over TLS
                // with our secret, and `state` already tied it to this browser.
                let nonce = match kind {
                    ProviderKind::LinkedIn => Nonce::IfPresent(callback.nonce),
                    _ => Nonce::Required(callback.nonce),
                };
                let claims =
                    verify_id_token(http, endpoints, &oidc.config.client_id, id_token(&tokens)?, nonce).await?;
                let email_verified = match kind {
                    ProviderKind::Microsoft => microsoft::email_verified(&claims),
                    _ => claims.email_verified,
                };
                Ok(Identity {
                    provider: kind,
                    subject: claims.sub,
                    email: claims.email,
                    email_verified,
                    name: claims.name,
                })
            }
            Self::Apple(apple) => {
                let services_id = &apple.config.services_id;
                let client =
                    client(services_id, Some(apple.client_secret()?), apple.endpoints.urls(), callback.redirect_uri)?;
                let tokens = exchange_code(http, &client, callback.code, verifier).await?;
                let nonce = Nonce::Required(callback.nonce);
                let claims = verify_id_token(http, &apple.endpoints, services_id, id_token(&tokens)?, nonce).await?;
                let name = callback.apple_user.and_then(super::apple::name_from_user_field).or(claims.name);
                Ok(Identity {
                    provider: kind,
                    subject: claims.sub,
                    email: claims.email,
                    email_verified: claims.email_verified,
                    name,
                })
            }
            Self::GitHub(api) | Self::Discord(api) => {
                let secret = Some(api.client_secret.clone());
                let client = client(&api.client_id, secret, api_urls(api), callback.redirect_uri)?;
                let tokens = exchange_code(http, &client, callback.code, verifier).await?;
                let access_token = tokens.access_token().secret();
                match self {
                    Self::GitHub(_) => github::identity(http, &api.endpoints.api_url, access_token).await,
                    _ => discord::identity(http, &api.endpoints.api_url, access_token).await,
                }
            }
        }
    }
}

fn api_urls(api: &ApiConfig) -> (&str, &str) {
    (&api.endpoints.authorize_url, &api.endpoints.token_url)
}

fn id_token(tokens: &OidcTokenResponse) -> Result<&str, AuthError> {
    tokens.extra_fields().id_token.as_deref().ok_or_else(|| AuthError::Token("no id_token in the answer".into()))
}

/// An OpenID Connect provider's URLs and keys.
pub struct OidcEndpoints {
    /// accepted `iss` values; `{tenantid}` stands for the token's `tid` (Microsoft)
    pub issuers: Vec<String>,
    pub authorize_url: String,
    pub token_url: String,
    pub jwks: Jwks,
}

impl OidcEndpoints {
    fn urls(&self) -> (&str, &str) {
        (&self.authorize_url, &self.token_url)
    }

    /// Whether `iss` is one of ours, with a `{tenantid}` template filled in from `tid`.
    pub fn accepts_issuer(&self, iss: &str, tid: Option<&str>) -> bool {
        self.issuers.iter().any(|issuer| match (issuer.contains("{tenantid}"), tid) {
            (false, _) => issuer == iss,
            (true, Some(tid)) => issuer.replace("{tenantid}", tid) == iss,
            (true, None) => false,
        })
    }
}

/// The token endpoint's answer: the standard fields plus `id_token` (absent from GitHub and Discord).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IdTokenField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
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
    (authorize_url, token_url): (&str, &str),
    redirect_uri: &str,
) -> Result<OidcClient, AuthError> {
    let bad = |what: &str, e: oauth2::url::ParseError| AuthError::Config(format!("{what}: {e}"));
    let mut client = oauth2::Client::new(ClientId::new(client_id.to_owned()))
        .set_auth_uri(AuthUrl::new(authorize_url.to_owned()).map_err(|e| bad("authorize url", e))?)
        .set_token_uri(TokenUrl::new(token_url.to_owned()).map_err(|e| bad("token url", e))?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_owned()).map_err(|e| bad("redirect url", e))?)
        // client_secret_post: every provider here takes it, and Apple's secret is a long JWT.
        .set_auth_type(AuthType::RequestBody);
    if let Some(secret) = secret {
        client = client.set_client_secret(ClientSecret::new(secret));
    }
    Ok(client)
}

fn authorize(
    client: &OidcClient,
    request: &AuthorizeRequest<'_>,
    scopes: &[&str],
    pkce: bool,
    nonce: bool,
    extra: &[(&str, &str)],
) -> String {
    let state = request.state.to_owned();
    let mut builder = client
        .authorize_url(move || CsrfToken::new(state))
        .add_scopes(scopes.iter().map(|s| Scope::new((*s).to_owned())));
    if pkce {
        builder = builder.set_pkce_challenge(request.pkce.clone());
    }
    if nonce {
        builder = builder.add_extra_param("nonce", request.nonce);
    }
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
    verifier: Option<&str>,
) -> Result<OidcTokenResponse, AuthError> {
    let call = |request| send(http.clone(), request);
    let mut request = client.exchange_code(AuthorizationCode::new(code.to_owned()));
    if let Some(verifier) = verifier {
        request = request.set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()));
    }
    request.request_async(&call).await.map_err(|e| match e {
        oauth2::RequestTokenError::ServerResponse(r) => AuthError::Exchange(r.to_string()),
        other => AuthError::Upstream(other.to_string()),
    })
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
    get_json(http.get(url), url).await
}

/// A GET with the provider's access token, for the profile APIs.
pub(crate) async fn fetch_api<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
    headers: &[(&str, &str)],
) -> Result<T, AuthError> {
    let mut request = http.get(url).bearer_auth(access_token);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    get_json(request, url).await
}

async fn get_json<T: serde::de::DeserializeOwned>(request: reqwest::RequestBuilder, url: &str) -> Result<T, AuthError> {
    let upstream = |e: reqwest::Error| AuthError::Upstream(format!("{url}: {e}"));
    request.send().await.and_then(reqwest::Response::error_for_status).map_err(upstream)?.json().await.map_err(upstream)
}

/// The id_token claims we use.
#[derive(Debug, Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub iss: String,
    /// Microsoft: the tenant the account lives in
    #[serde(default)]
    pub tid: Option<String>,
    /// Microsoft: the tenant has verified that it owns the e-mail's domain (an optional claim)
    #[serde(default, deserialize_with = "lenient_bool")]
    pub xms_edov: bool,
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

/// Whether the id_token must carry the nonce we sent.
#[derive(Clone, Copy, Debug)]
pub enum Nonce<'a> {
    Required(&'a str),
    /// checked when it is there (LinkedIn)
    IfPresent(&'a str),
}

pub async fn verify_id_token(
    http: &reqwest::Client,
    endpoints: &OidcEndpoints,
    client_id: &str,
    id_token: &str,
    nonce: Nonce<'_>,
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
    // `iss` must be there; which values count is checked below, because Microsoft's depends on `tid`.
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = jsonwebtoken::decode::<IdClaims>(id_token, &key, &validation).map_err(bad)?.claims;
    if !endpoints.accepts_issuer(&claims.iss, claims.tid.as_deref()) {
        return Err(AuthError::Token(format!("issuer {:?} not accepted", claims.iss)));
    }
    let matches =
        |expected: &str| claims.nonce.as_deref().is_some_and(|got| super::same(got.as_bytes(), expected.as_bytes()));
    match nonce {
        Nonce::Required(expected) if matches(expected) => Ok(claims),
        Nonce::IfPresent(_) if claims.nonce.is_none() => Ok(claims),
        Nonce::IfPresent(expected) if matches(expected) => Ok(claims),
        _ => Err(AuthError::Token("nonce does not match".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_verified_is_lenient() {
        let parse = |json: &str| serde_json::from_str::<IdClaims>(json).unwrap().email_verified;
        assert!(parse(r#"{"sub":"1","iss":"i","email_verified":true}"#));
        assert!(parse(r#"{"sub":"1","iss":"i","email_verified":"true"}"#));
        assert!(!parse(r#"{"sub":"1","iss":"i","email_verified":"false"}"#));
        assert!(!parse(r#"{"sub":"1","iss":"i","email_verified":1}"#));
        assert!(!parse(r#"{"sub":"1","iss":"i"}"#));
    }

    #[test]
    fn provider_names() {
        assert_eq!(ProviderKind::parse("apple"), Some(ProviderKind::Apple));
        assert_eq!(ProviderKind::parse("Google"), None);
        for kind in ProviderKind::ALL {
            assert_eq!(ProviderKind::parse(kind.as_str()), Some(kind));
            // The wire name is the same as the one in URLs and the database.
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
    }

    #[test]
    fn issuer_templates_take_the_tenant() {
        let endpoints = OidcEndpoints {
            issuers: vec!["https://accounts.google.com".into(), "https://login.example/{tenantid}/v2.0".into()],
            authorize_url: String::new(),
            token_url: String::new(),
            jwks: Jwks::new(String::new()),
        };
        assert!(endpoints.accepts_issuer("https://accounts.google.com", None));
        assert!(endpoints.accepts_issuer("https://login.example/t1/v2.0", Some("t1")));
        assert!(!endpoints.accepts_issuer("https://login.example/t2/v2.0", Some("t1")));
        assert!(!endpoints.accepts_issuer("https://login.example/{tenantid}/v2.0", None));
        assert!(!endpoints.accepts_issuer("https://evil.example", Some("t1")));
    }
}
