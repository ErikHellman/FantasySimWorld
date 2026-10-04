//! Configuration, read once from the environment at start.

use std::net::SocketAddr;
use std::path::PathBuf;

use fsw_brain::settings::{ApiKey, ConversationMode, LlmSettings, ReasoningMode, WireProtocol};

use crate::api::wire::SignInProvider;

/// How viewers are let in. The only mode without sign-in, [`AuthMode::Off`],
/// refuses to start anywhere but on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// No sign-in: everyone is an admin. For local development only, so it needs
    /// a loopback bind address and an `http://localhost` (or loopback IP) origin.
    Off,
    /// One shared secret, for local work and the first deployments. Browsers
    /// post it to `/auth/token` once and get a cookie; everyone is an admin.
    Token(String),
    /// Sign-in with Google, Apple, Microsoft, LinkedIn, GitHub and/or Discord, limited to the allowlist.
    Oidc(Box<OidcConfig>),
}

/// The providers `AUTH_PROVIDERS` turned on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OidcConfig {
    /// in `AUTH_PROVIDERS` order, which is also the order of the buttons on the login page
    pub providers: Vec<ProviderConfig>,
}

impl OidcConfig {
    /// The settings of one provider, if it is on.
    pub fn get_mut(&mut self, kind: SignInProvider) -> Option<&mut ProviderConfig> {
        self.providers.iter_mut().find(|p| p.kind() == kind)
    }
}

/// One provider's settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderConfig {
    Google(DiscoveryConfig),
    Apple(AppleConfig),
    Microsoft(DiscoveryConfig),
    LinkedIn(DiscoveryConfig),
    GitHub(ApiConfig),
    Discord(ApiConfig),
}

impl ProviderConfig {
    pub fn kind(&self) -> SignInProvider {
        match self {
            Self::Google(_) => SignInProvider::Google,
            Self::Apple(_) => SignInProvider::Apple,
            Self::Microsoft(_) => SignInProvider::Microsoft,
            Self::LinkedIn(_) => SignInProvider::LinkedIn,
            Self::GitHub(_) => SignInProvider::GitHub,
            Self::Discord(_) => SignInProvider::Discord,
        }
    }
}

pub const GOOGLE_DISCOVERY_URL: &str = "https://accounts.google.com/.well-known/openid-configuration";
pub const LINKEDIN_DISCOVERY_URL: &str = "https://www.linkedin.com/oauth/.well-known/openid-configuration";

/// Microsoft's discovery document for a tenant: `common`, `consumers`, `organizations`, a tenant id or a domain.
pub fn microsoft_discovery_url(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/v2.0/.well-known/openid-configuration")
}

/// An OpenID Connect provider found through its discovery document (Google, Microsoft, LinkedIn).
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    /// `<PROVIDER>_CLIENT_ID`
    pub client_id: String,
    /// `<PROVIDER>_CLIENT_SECRET`
    pub client_secret: String,
    /// the provider's discovery document; only tests (and `MICROSOFT_TENANT`) change it
    pub discovery_url: String,
}

impl std::fmt::Debug for DiscoveryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryConfig")
            .field("client_id", &self.client_id)
            .field("discovery_url", &self.discovery_url)
            .finish_non_exhaustive()
    }
}

/// A plain OAuth2 provider whose profile API says who signed in (GitHub, Discord).
#[derive(Clone, PartialEq, Eq)]
pub struct ApiConfig {
    /// `<PROVIDER>_CLIENT_ID`
    pub client_id: String,
    /// `<PROVIDER>_CLIENT_SECRET`
    pub client_secret: String,
    /// the provider's URLs; only tests change them
    pub endpoints: ApiEndpoints,
}

impl std::fmt::Debug for ApiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiConfig")
            .field("client_id", &self.client_id)
            .field("endpoints", &self.endpoints)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiEndpoints {
    pub authorize_url: String,
    pub token_url: String,
    /// the profile API, without a trailing slash
    pub api_url: String,
}

impl ApiEndpoints {
    pub fn github() -> Self {
        Self {
            authorize_url: "https://github.com/login/oauth/authorize".into(),
            token_url: "https://github.com/login/oauth/access_token".into(),
            api_url: "https://api.github.com".into(),
        }
    }

    pub fn discord() -> Self {
        Self {
            authorize_url: "https://discord.com/oauth2/authorize".into(),
            token_url: "https://discord.com/api/oauth2/token".into(),
            api_url: "https://discord.com/api".into(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AppleConfig {
    /// `APPLE_TEAM_ID`
    pub team_id: String,
    /// `APPLE_KEY_ID`, the id of the Sign in with Apple key
    pub key_id: String,
    /// `APPLE_SERVICES_ID`, used as the OAuth client id
    pub services_id: String,
    /// `APPLE_PRIVATE_KEY`, the PEM contents of the `.p8` file
    pub private_key_pem: String,
    /// Apple's URLs; only tests change them
    pub endpoints: AppleEndpoints,
}

impl std::fmt::Debug for AppleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppleConfig")
            .field("team_id", &self.team_id)
            .field("key_id", &self.key_id)
            .field("services_id", &self.services_id)
            .field("endpoints", &self.endpoints)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppleEndpoints {
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    pub jwks_url: String,
}

impl Default for AppleEndpoints {
    fn default() -> Self {
        Self {
            issuer: "https://appleid.apple.com".into(),
            authorize_url: "https://appleid.apple.com/auth/authorize".into(),
            token_url: "https://appleid.apple.com/auth/token".into(),
            jwks_url: "https://appleid.apple.com/auth/keys".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// `BIND_ADDR`, default `0.0.0.0:8080`, or `127.0.0.1:8080` with `AUTH_MODE=off` (`PORT` alone also works)
    pub bind: SocketAddr,
    /// `PUBLIC_ORIGIN`, e.g. `https://world.example.com`; decides cookie flags and the CSRF origin check
    pub public_origin: String,
    /// `STATIC_DIR`, the Vite build; default `dist`
    pub static_dir: PathBuf,
    /// `DATA_DIR`, where SQLite lives; default `data`
    pub data_dir: PathBuf,
    /// `AUTH_MODE`: `token` (with `AUTH_TOKEN`), `oidc` (with `AUTH_PROVIDERS` and the provider secrets)
    /// or `off` (no sign-in; local development only)
    pub auth: AuthMode,
    /// `ADMIN_EMAILS`, comma-separated: put on the allowlist as admins at every start
    pub admin_emails: Vec<String>,
    /// `TRUST_PROXY=true`: take the client address from `Fly-Client-IP` / `X-Forwarded-For`
    /// (only behind a proxy that sets them; otherwise anyone could pick their own address)
    pub trust_proxy: bool,
    /// `LOG_FORMAT=pretty` for humans; JSON lines otherwise
    pub pretty_logs: bool,
    /// `INITIAL_SPEED`: 0, 1 (default), 2 or 4. 0 starts paused, which leaves only
    /// keep-alives on the stream (for checking a proxy's idle timeout).
    pub initial_speed: u8,
    /// The language model, from the `LLM_*` variables (see `llm_from`). Off unless
    /// `LLM_BASE_URL` and `LLM_MODEL` are both set; nothing about it can be changed from a browser.
    pub llm: LlmSettings,
    /// `LLM_API_KEY`: the model endpoint's key, if it needs one (masked in `Debug`)
    pub llm_api_key: ApiKey,
    /// `CREDITS_PER_MINUTE`: what a viewer earns for a minute of watching with the tab in front;
    /// default 2, and 0 turns credits off altogether
    pub credits_per_minute: u32,
    /// `CREDITS_CAP`: the most credits watching and prayer can build up; default 400
    pub credits_cap: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_vars(std::env::vars())
    }

    /// Builds the configuration from `(name, value)` pairs, so tests need not touch the process environment.
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Result<Self, String> {
        let vars: std::collections::HashMap<String, String> = vars.into_iter().collect();
        let get = |name: &str| vars.get(name).map(|v| v.trim()).filter(|v| !v.is_empty());

        let auth = match get("AUTH_MODE").unwrap_or("token") {
            "off" => AuthMode::Off,
            "token" => {
                let token = get("AUTH_TOKEN").ok_or("AUTH_MODE=token needs AUTH_TOKEN (any long random string)")?;
                if token.len() < 16 {
                    return Err("AUTH_TOKEN must be at least 16 characters".into());
                }
                AuthMode::Token(token.to_owned())
            }
            "oidc" => AuthMode::Oidc(Box::new(oidc_from(&get)?)),
            other => return Err(format!("AUTH_MODE must be token, oidc or off, got {other:?}")),
        };
        let open = auth == AuthMode::Off;

        // Without sign-in, listen on this machine only unless told otherwise (and then refuse anything else).
        let host = if open { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
        let bind = match (get("BIND_ADDR"), get("PORT")) {
            (Some(addr), _) => addr.parse().map_err(|e| format!("BIND_ADDR {addr:?}: {e}"))?,
            (None, Some(port)) => {
                let port: u16 = port.parse().map_err(|e| format!("PORT {port:?}: {e}"))?;
                SocketAddr::from((host, port))
            }
            (None, None) => SocketAddr::from((host, 8080)),
        };
        if open && !bind.ip().is_loopback() {
            return Err(format!(
                "AUTH_MODE=off listens on this machine only; BIND_ADDR {bind} is not a loopback address"
            ));
        }

        let public_origin = get("PUBLIC_ORIGIN").unwrap_or("http://localhost:5173").trim_end_matches('/').to_owned();
        if !(public_origin.starts_with("https://") || public_origin.starts_with("http://")) {
            return Err(format!("PUBLIC_ORIGIN must start with https:// or http://, got {public_origin:?}"));
        }
        if open && !local_origin(&public_origin) {
            return Err(format!(
                "AUTH_MODE=off is for local development; PUBLIC_ORIGIN must be http://localhost (or a loopback IP), got {public_origin:?}"
            ));
        }

        let mut admin_emails = Vec::new();
        for email in get("ADMIN_EMAILS").unwrap_or("").split(',').map(str::trim).filter(|e| !e.is_empty()) {
            if !plausible_email(email) {
                return Err(format!("ADMIN_EMAILS: {email:?} is not an e-mail address"));
            }
            admin_emails.push(email.to_lowercase());
        }

        let trust_proxy = match get("TRUST_PROXY").map(str::to_ascii_lowercase).as_deref() {
            None | Some("0" | "false" | "no") => false,
            Some("1" | "true" | "yes") => true,
            Some(other) => return Err(format!("TRUST_PROXY must be true or false, got {other:?}")),
        };

        let initial_speed = match get("INITIAL_SPEED") {
            None => 1,
            Some(text) => match text.parse::<u8>() {
                Ok(speed) if crate::world_task::SPEEDS.contains(&speed) => speed,
                _ => return Err(format!("INITIAL_SPEED must be 0, 1, 2 or 4, got {text:?}")),
            },
        };

        let llm = llm_from(&get)?;
        let whole = |name: &str, default: u32, range: std::ops::RangeInclusive<u32>| match get(name) {
            None => Ok(default),
            Some(text) => match text.parse::<u32>() {
                Ok(value) if range.contains(&value) => Ok(value),
                _ => Err(format!("{name} must be a number from {} to {}, got {text:?}", range.start(), range.end())),
            },
        };
        let credits_per_minute = whole("CREDITS_PER_MINUTE", 2, 0..=600)?;
        let credits_cap = whole("CREDITS_CAP", 400, 1..=1_000_000)?;

        Ok(Self {
            bind,
            public_origin,
            static_dir: get("STATIC_DIR").unwrap_or("dist").into(),
            data_dir: get("DATA_DIR").unwrap_or("data").into(),
            auth,
            admin_emails,
            trust_proxy,
            pretty_logs: get("LOG_FORMAT") == Some("pretty"),
            initial_speed,
            llm,
            llm_api_key: ApiKey::new(get("LLM_API_KEY").unwrap_or("")),
            credits_per_minute,
            credits_cap,
        })
    }

    /// Cookies get `Secure` (and the `__Host-` / `__Secure-` prefixes) only when served over https.
    pub fn secure_cookies(&self) -> bool {
        self.public_origin.starts_with("https://")
    }
}

/// Good enough to catch a typo: something@something, no spaces.
pub fn plausible_email(email: &str) -> bool {
    email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email.split_once('@').is_some_and(|(user, host)| !user.is_empty() && !host.is_empty() && !host.contains('@'))
}

/// An `http://` origin on this machine: `localhost`, `127.0.0.1` or `[::1]`, with any port.
pub fn local_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else { return false };
    if rest.contains('/') || rest.contains('@') {
        return false;
    }
    let host = match rest.strip_prefix('[') {
        Some(bracketed) => match bracketed.split_once(']') {
            Some((host, port)) if port.is_empty() || port.starts_with(':') => host,
            _ => return false,
        },
        None => rest.split_once(':').map_or(rest, |(host, _)| host),
    };
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The names `AUTH_PROVIDERS` accepts, for error messages.
const PROVIDER_NAMES: &str = "google, apple, microsoft, linkedin, github, discord";

fn oidc_from<'a>(get: &impl Fn(&str) -> Option<&'a str>) -> Result<OidcConfig, String> {
    let need = |name: &str, provider: &str| {
        get(name).map(str::to_owned).ok_or_else(|| format!("AUTH_PROVIDERS includes {provider}, which needs {name}"))
    };
    let discovery = |prefix: &str, provider: &str, discovery_url: String| -> Result<DiscoveryConfig, String> {
        Ok(DiscoveryConfig {
            client_id: need(&format!("{prefix}_CLIENT_ID"), provider)?,
            client_secret: need(&format!("{prefix}_CLIENT_SECRET"), provider)?,
            discovery_url,
        })
    };
    let api = |prefix: &str, provider: &str, endpoints: ApiEndpoints| -> Result<ApiConfig, String> {
        Ok(ApiConfig {
            client_id: need(&format!("{prefix}_CLIENT_ID"), provider)?,
            client_secret: need(&format!("{prefix}_CLIENT_SECRET"), provider)?,
            endpoints,
        })
    };
    let names = get("AUTH_PROVIDERS")
        .ok_or_else(|| format!("AUTH_MODE=oidc needs AUTH_PROVIDERS, a comma-separated list of {PROVIDER_NAMES}"))?;
    let mut oidc = OidcConfig::default();
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let provider = match name {
            "google" => ProviderConfig::Google(discovery("GOOGLE", name, GOOGLE_DISCOVERY_URL.into())?),
            "apple" => {
                // Platforms that cannot hold newlines in a secret get "\n" written out instead.
                let pem = need("APPLE_PRIVATE_KEY", name)?.replace("\\n", "\n");
                jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes())
                    .map_err(|e| format!("APPLE_PRIVATE_KEY is not the PEM of a .p8 key: {e}"))?;
                ProviderConfig::Apple(AppleConfig {
                    team_id: need("APPLE_TEAM_ID", name)?,
                    key_id: need("APPLE_KEY_ID", name)?,
                    services_id: need("APPLE_SERVICES_ID", name)?,
                    private_key_pem: pem,
                    endpoints: AppleEndpoints::default(),
                })
            }
            "microsoft" => {
                let tenant = get("MICROSOFT_TENANT").unwrap_or("common");
                // It goes into a URL path, so only what tenant ids and domains are made of.
                if !tenant.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
                    return Err(format!("MICROSOFT_TENANT {tenant:?} is not a tenant id, domain or common"));
                }
                ProviderConfig::Microsoft(discovery("MICROSOFT", name, microsoft_discovery_url(tenant))?)
            }
            "linkedin" => ProviderConfig::LinkedIn(discovery("LINKEDIN", name, LINKEDIN_DISCOVERY_URL.into())?),
            "github" => ProviderConfig::GitHub(api("GITHUB", name, ApiEndpoints::github())?),
            "discord" => ProviderConfig::Discord(api("DISCORD", name, ApiEndpoints::discord())?),
            other => return Err(format!("AUTH_PROVIDERS: unknown provider {other:?} ({PROVIDER_NAMES})")),
        };
        if oidc.providers.iter().any(|p| p.kind() == provider.kind()) {
            return Err(format!("AUTH_PROVIDERS names {name} twice"));
        }
        oidc.providers.push(provider);
    }
    if oidc.providers.is_empty() {
        return Err("AUTH_PROVIDERS names no provider".into());
    }
    Ok(oidc)
}

/// The language model's settings. Every value has the same default and range the
/// admin panel used to offer; anything else stops the server at start.
fn llm_from<'a>(get: &impl Fn(&str) -> Option<&'a str>) -> Result<LlmSettings, String> {
    fn number<'a, T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy>(
        get: &impl Fn(&str) -> Option<&'a str>,
        name: &str,
        default: T,
        range: std::ops::RangeInclusive<T>,
    ) -> Result<T, String> {
        let Some(text) = get(name) else { return Ok(default) };
        match text.parse::<T>() {
            Ok(value) if range.contains(&value) => Ok(value),
            _ => Err(format!("{name} must be a number from {} to {}, got {text:?}", range.start(), range.end())),
        }
    }
    fn choice<'a, T: serde::de::DeserializeOwned + Default>(
        get: &impl Fn(&str) -> Option<&'a str>,
        name: &str,
        names: &str,
    ) -> Result<T, String> {
        match get(name) {
            None => Ok(T::default()),
            Some(text) => serde_json::from_value(serde_json::Value::String(text.to_ascii_lowercase()))
                .map_err(|_| format!("{name} must be one of {names}, got {text:?}")),
        }
    }

    let defaults = LlmSettings::default();
    let base_url = get("LLM_BASE_URL").unwrap_or("").trim_end_matches('/').to_owned();
    let model = get("LLM_MODEL").unwrap_or("").to_owned();
    match (base_url.is_empty(), model.is_empty()) {
        (false, true) => return Err("LLM_BASE_URL is set but LLM_MODEL is not; set both or neither".into()),
        (true, false) => return Err("LLM_MODEL is set but LLM_BASE_URL is not; set both or neither".into()),
        _ => {}
    }
    if !(base_url.is_empty() || base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(format!("LLM_BASE_URL must start with https:// or http://, got {base_url:?}"));
    }
    let protocol: WireProtocol = choice(get, "LLM_PROTOCOL", "auto, tools or json")?;
    let conversations: ConversationMode = choice(get, "LLM_CONVERSATIONS", "auto, batched or turns")?;
    let reasoning: ReasoningMode = choice(get, "LLM_REASONING", "off, low or default")?;
    Ok(LlmSettings {
        enabled: !base_url.is_empty(),
        base_url,
        model,
        concurrency: number(get, "LLM_CONCURRENCY", defaults.concurrency, 1..=16)?,
        protocol,
        conversations,
        reasoning,
        temperature: number(get, "LLM_TEMPERATURE", defaults.temperature, 0.0..=2.0)?,
        max_tokens: number(get, "LLM_MAX_TOKENS", defaults.max_tokens, 100..=4000)?,
        // Platforms that cannot hold newlines in a value get "\n" written out instead.
        headers: get("LLM_HEADERS").unwrap_or("").replace("\\n", "\n"),
        requests_per_minute: number(get, "LLM_REQUESTS_PER_MINUTE", defaults.requests_per_minute, 0..=u32::MAX)?,
        max_requests_per_day: number(get, "LLM_MAX_REQUESTS_PER_DAY", defaults.max_requests_per_day, 0..=u64::MAX)?,
        max_tokens_per_day: number(get, "LLM_MAX_TOKENS_PER_DAY", defaults.max_tokens_per_day, 0..=u64::MAX)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn defaults_need_only_a_token() {
        let config = Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef")])).unwrap();
        assert_eq!(config.bind, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.public_origin, "http://localhost:5173");
        assert_eq!(config.auth, AuthMode::Token("0123456789abcdef".into()));
        assert!(!config.secure_cookies());
        assert_eq!(config.initial_speed, 1);
        assert!(config.admin_emails.is_empty());
        assert!(!config.trust_proxy);
    }

    #[test]
    fn credits_have_a_rate_and_a_cap() {
        let with =
            |name: &str, value: &str| Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef"), (name, value)]));
        let config = Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef")])).unwrap();
        assert_eq!((config.credits_per_minute, config.credits_cap), (2, 400));
        assert_eq!(with("CREDITS_PER_MINUTE", "0").unwrap().credits_per_minute, 0);
        assert_eq!(with("CREDITS_CAP", "50").unwrap().credits_cap, 50);
        assert!(with("CREDITS_PER_MINUTE", "lots").unwrap_err().contains("CREDITS_PER_MINUTE"));
        assert!(with("CREDITS_CAP", "0").unwrap_err().contains("CREDITS_CAP"));
    }

    #[test]
    fn initial_speed_is_one_of_the_speeds() {
        let with =
            |speed: &str| Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef"), ("INITIAL_SPEED", speed)]));
        assert_eq!(with("0").unwrap().initial_speed, 0);
        assert_eq!(with("4").unwrap().initial_speed, 4);
        assert!(with("3").is_err());
    }

    #[test]
    fn port_and_origin() {
        let config = Config::from_vars(vars(&[
            ("PORT", "3000"),
            ("PUBLIC_ORIGIN", "https://world.example.com/"),
            ("AUTH_TOKEN", "0123456789abcdef"),
        ]))
        .unwrap();
        assert_eq!(config.bind.port(), 3000);
        assert_eq!(config.public_origin, "https://world.example.com");
        assert!(config.secure_cookies());
    }

    #[test]
    fn refuses_to_run_open() {
        assert!(Config::from_vars(vars(&[])).is_err());
        assert!(Config::from_vars(vars(&[("AUTH_TOKEN", "short")])).is_err());
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "nope"), ("AUTH_TOKEN", "0123456789abcdef")])).is_err());
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "oidc")])).is_err());
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "oidc"), ("AUTH_PROVIDERS", ",")])).is_err());
    }

    #[test]
    fn oidc_needs_each_providers_secrets() {
        let google = [
            ("AUTH_MODE", "oidc"),
            ("AUTH_PROVIDERS", "google"),
            ("GOOGLE_CLIENT_ID", "id.apps.googleusercontent.com"),
            ("GOOGLE_CLIENT_SECRET", "shh"),
            ("ADMIN_EMAILS", " Boss@Example.com, , second@example.com"),
            ("TRUST_PROXY", "true"),
        ];
        let config = Config::from_vars(vars(&google)).unwrap();
        let AuthMode::Oidc(oidc) = &config.auth else { panic!("oidc") };
        let [ProviderConfig::Google(only)] = oidc.providers.as_slice() else { panic!("only Google") };
        assert_eq!(only.discovery_url, GOOGLE_DISCOVERY_URL);
        assert!(!format!("{config:?}").contains("shh"), "secrets stay out of logs");
        assert_eq!(config.admin_emails, ["boss@example.com", "second@example.com"]);
        assert!(config.trust_proxy);

        assert!(Config::from_vars(vars(&google[..3])).is_err(), "no client secret");
        let mut apple = google.to_vec();
        apple[1] = ("AUTH_PROVIDERS", "google,apple");
        assert!(Config::from_vars(vars(&apple)).is_err(), "no Apple settings");
        apple.extend([
            ("APPLE_TEAM_ID", "TEAM"),
            ("APPLE_KEY_ID", "KEY"),
            ("APPLE_SERVICES_ID", "com.example.world"),
            ("APPLE_PRIVATE_KEY", "not a key"),
        ]);
        assert!(Config::from_vars(vars(&apple)).unwrap_err().contains("APPLE_PRIVATE_KEY"));
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "oidc"), ("AUTH_PROVIDERS", "facebook")])).is_err());
    }

    #[test]
    fn every_provider_reads_its_own_variables_in_order() {
        let all = [
            ("AUTH_MODE", "oidc"),
            ("AUTH_PROVIDERS", "github, microsoft,linkedin,discord"),
            ("GITHUB_CLIENT_ID", "gh-id"),
            ("GITHUB_CLIENT_SECRET", "shh-gh"),
            ("MICROSOFT_CLIENT_ID", "ms-id"),
            ("MICROSOFT_CLIENT_SECRET", "shh-ms"),
            ("LINKEDIN_CLIENT_ID", "li-id"),
            ("LINKEDIN_CLIENT_SECRET", "shh-li"),
            ("DISCORD_CLIENT_ID", "dc-id"),
            ("DISCORD_CLIENT_SECRET", "shh-dc"),
        ];
        let config = Config::from_vars(vars(&all)).unwrap();
        let AuthMode::Oidc(oidc) = &config.auth else { panic!("oidc") };
        let kinds: Vec<_> = oidc.providers.iter().map(ProviderConfig::kind).collect();
        use SignInProvider::*;
        assert_eq!(kinds, [GitHub, Microsoft, LinkedIn, Discord]);
        let ProviderConfig::Microsoft(microsoft) = &oidc.providers[1] else { panic!("Microsoft") };
        assert_eq!(microsoft.discovery_url, microsoft_discovery_url("common"));
        let ProviderConfig::GitHub(github) = &oidc.providers[0] else { panic!("GitHub") };
        assert_eq!((github.client_id.as_str(), github.endpoints.clone()), ("gh-id", ApiEndpoints::github()));
        let logged = format!("{config:?}");
        assert!(!logged.contains("shh"), "secrets stay out of logs: {logged}");

        for (i, (name, _)) in all.iter().enumerate().skip(2) {
            let without: Vec<_> = all.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, v)| *v).collect();
            let error = Config::from_vars(vars(&without)).unwrap_err();
            assert!(error.contains(name), "{error}");
        }

        let mut tenant = all.to_vec();
        tenant.push(("MICROSOFT_TENANT", "contoso.onmicrosoft.com"));
        let config = Config::from_vars(vars(&tenant)).unwrap();
        let AuthMode::Oidc(oidc) = &config.auth else { panic!("oidc") };
        let ProviderConfig::Microsoft(microsoft) = &oidc.providers[1] else { panic!("Microsoft") };
        assert!(microsoft.discovery_url.contains("/contoso.onmicrosoft.com/v2.0/"));
        *tenant.last_mut().unwrap() = ("MICROSOFT_TENANT", "evil.example/x?");
        assert!(Config::from_vars(vars(&tenant)).unwrap_err().contains("MICROSOFT_TENANT"));

        let mut twice = all.to_vec();
        twice[1] = ("AUTH_PROVIDERS", "github,discord,github");
        assert!(Config::from_vars(vars(&twice)).unwrap_err().contains("twice"));
    }

    #[test]
    fn admin_emails_and_proxy_flag_are_checked() {
        let with =
            |name: &str, value: &str| Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef"), (name, value)]));
        assert!(with("ADMIN_EMAILS", "not-an-email").is_err());
        assert!(with("TRUST_PROXY", "maybe").is_err());
        assert!(!with("TRUST_PROXY", "0").unwrap().trust_proxy);
    }

    #[test]
    fn the_model_is_off_without_llm_settings() {
        let config = Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef")])).unwrap();
        assert_eq!(config.llm, LlmSettings::default());
        assert!(!config.llm.enabled);
        assert!(config.llm_api_key.is_empty());
    }

    #[test]
    fn the_model_comes_from_the_environment() {
        let config = Config::from_vars(vars(&[
            ("AUTH_TOKEN", "0123456789abcdef"),
            ("LLM_BASE_URL", " https://api.example.com/v1/ "),
            ("LLM_MODEL", "villager-1"),
            ("LLM_API_KEY", "sk-secret-9876"),
            ("LLM_CONCURRENCY", "4"),
            ("LLM_REQUESTS_PER_MINUTE", "0"),
            ("LLM_MAX_REQUESTS_PER_DAY", "20000"),
            ("LLM_MAX_TOKENS_PER_DAY", "5000000"),
            ("LLM_PROTOCOL", "JSON"),
            ("LLM_CONVERSATIONS", "turns"),
            ("LLM_REASONING", "low"),
            ("LLM_TEMPERATURE", "0.5"),
            ("LLM_MAX_TOKENS", "800"),
            ("LLM_HEADERS", "X-One: 1\\nX-Two: 2"),
        ]))
        .unwrap();
        let llm = &config.llm;
        assert!(llm.enabled);
        assert_eq!(llm.base_url, "https://api.example.com/v1");
        assert_eq!(llm.model, "villager-1");
        assert_eq!(config.llm_api_key.expose(), "sk-secret-9876");
        assert_eq!((llm.concurrency, llm.requests_per_minute), (4, 0));
        assert_eq!((llm.max_requests_per_day, llm.max_tokens_per_day), (20_000, 5_000_000));
        assert_eq!(llm.protocol, WireProtocol::Json);
        assert_eq!(llm.conversations, ConversationMode::Turns);
        assert_eq!(llm.reasoning, ReasoningMode::Low);
        assert_eq!((llm.temperature, llm.max_tokens), (0.5, 800));
        assert_eq!(fsw_brain::settings::parse_headers(&llm.headers).len(), 2);
        assert!(!format!("{config:?}").contains("sk-secret"), "the key stays out of logs");
    }

    #[test]
    fn bad_llm_settings_stop_the_server() {
        let with = |pairs: &[(&str, &str)]| {
            let mut all = vec![("AUTH_TOKEN", "0123456789abcdef")];
            all.extend_from_slice(pairs);
            Config::from_vars(vars(&all))
        };
        assert!(with(&[("LLM_MODEL", "m")]).unwrap_err().contains("LLM_BASE_URL"));
        assert!(with(&[("LLM_BASE_URL", "http://localhost:1234/v1")]).unwrap_err().contains("LLM_MODEL"));
        assert!(with(&[("LLM_BASE_URL", "localhost:1234"), ("LLM_MODEL", "m")]).is_err());
        assert!(with(&[("LLM_CONCURRENCY", "0")]).unwrap_err().contains("LLM_CONCURRENCY"));
        assert!(with(&[("LLM_CONCURRENCY", "17")]).is_err());
        assert!(with(&[("LLM_TEMPERATURE", "3")]).is_err());
        assert!(with(&[("LLM_TEMPERATURE", "NaN")]).is_err());
        assert!(with(&[("LLM_MAX_TOKENS", "10")]).is_err());
        assert!(with(&[("LLM_REQUESTS_PER_MINUTE", "-1")]).is_err());
        assert!(with(&[("LLM_PROTOCOL", "xml")]).unwrap_err().contains("LLM_PROTOCOL"));
        assert!(with(&[("LLM_REASONING", "high")]).is_err());
        assert!(with(&[("LLM_CONVERSATIONS", "maybe")]).is_err());
    }

    #[test]
    fn off_stays_on_this_machine() {
        let off = |extra: &[(&str, &str)]| {
            let mut pairs = vec![("AUTH_MODE", "off")];
            pairs.extend_from_slice(extra);
            Config::from_vars(vars(&pairs))
        };
        let config = off(&[]).unwrap();
        assert_eq!(config.auth, AuthMode::Off);
        assert_eq!(config.bind, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(off(&[("PORT", "3000")]).unwrap().bind, "127.0.0.1:3000".parse().unwrap());
        assert!(off(&[("BIND_ADDR", "[::1]:8080"), ("PUBLIC_ORIGIN", "http://127.0.0.1:8080")]).is_ok());

        assert!(off(&[("BIND_ADDR", "0.0.0.0:8080")]).is_err());
        assert!(off(&[("PUBLIC_ORIGIN", "https://world.example.com")]).is_err());
        assert!(off(&[("PUBLIC_ORIGIN", "http://192.168.1.5:5173")]).is_err());
    }

    #[test]
    fn local_origins() {
        assert!(local_origin("http://localhost:5173"));
        assert!(local_origin("http://LOCALHOST"));
        assert!(local_origin("http://127.0.0.1:8080"));
        assert!(local_origin("http://[::1]:8080"));
        assert!(!local_origin("https://localhost:5173"));
        assert!(!local_origin("http://localhost.example.com"));
        assert!(!local_origin("http://localhost@evil.example"));
        assert!(!local_origin("http://10.0.0.1"));
        assert!(!local_origin("http://[::1]evil"));
    }

    #[test]
    fn emails() {
        assert!(plausible_email("a@b.c"));
        assert!(!plausible_email("a b@c"));
        assert!(!plausible_email("@b"));
        assert!(!plausible_email("a@"));
        assert!(!plausible_email("a@b@c"));
    }
}
