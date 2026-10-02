//! Configuration, read once from the environment at start.

use std::net::SocketAddr;
use std::path::PathBuf;

use fsw_brain::settings::{ApiKey, ConversationMode, LlmSettings, ReasoningMode, WireProtocol};

/// How viewers are let in. There is deliberately no mode without sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// One shared secret, for local work and the first deployments. Browsers
    /// post it to `/auth/token` once and get a cookie; everyone is an admin.
    Token(String),
    /// Google and/or Apple sign-in, limited to the allowlist.
    Oidc(Box<OidcConfig>),
}

/// The providers `AUTH_PROVIDERS` turned on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OidcConfig {
    pub google: Option<GoogleConfig>,
    pub apple: Option<AppleConfig>,
}

pub const GOOGLE_DISCOVERY_URL: &str = "https://accounts.google.com/.well-known/openid-configuration";

#[derive(Clone, PartialEq, Eq)]
pub struct GoogleConfig {
    /// `GOOGLE_CLIENT_ID`
    pub client_id: String,
    /// `GOOGLE_CLIENT_SECRET`
    pub client_secret: String,
    /// Google's discovery document; only tests change it
    pub discovery_url: String,
}

impl std::fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleConfig")
            .field("client_id", &self.client_id)
            .field("discovery_url", &self.discovery_url)
            .finish_non_exhaustive()
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
    /// `BIND_ADDR`, default `0.0.0.0:8080` (`PORT` alone also works)
    pub bind: SocketAddr,
    /// `PUBLIC_ORIGIN`, e.g. `https://world.example.com`; decides cookie flags and the CSRF origin check
    pub public_origin: String,
    /// `STATIC_DIR`, the Vite build; default `dist`
    pub static_dir: PathBuf,
    /// `DATA_DIR`, where SQLite lives; default `data`
    pub data_dir: PathBuf,
    /// `AUTH_MODE`: `token` (with `AUTH_TOKEN`) or `oidc` (with `AUTH_PROVIDERS` and the provider secrets)
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
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_vars(std::env::vars())
    }

    /// Builds the configuration from `(name, value)` pairs, so tests need not touch the process environment.
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Result<Self, String> {
        let vars: std::collections::HashMap<String, String> = vars.into_iter().collect();
        let get = |name: &str| vars.get(name).map(|v| v.trim()).filter(|v| !v.is_empty());

        let bind = match (get("BIND_ADDR"), get("PORT")) {
            (Some(addr), _) => addr.parse().map_err(|e| format!("BIND_ADDR {addr:?}: {e}"))?,
            (None, Some(port)) => {
                let port: u16 = port.parse().map_err(|e| format!("PORT {port:?}: {e}"))?;
                SocketAddr::from(([0, 0, 0, 0], port))
            }
            (None, None) => SocketAddr::from(([0, 0, 0, 0], 8080)),
        };

        let public_origin = get("PUBLIC_ORIGIN").unwrap_or("http://localhost:5173").trim_end_matches('/').to_owned();
        if !(public_origin.starts_with("https://") || public_origin.starts_with("http://")) {
            return Err(format!("PUBLIC_ORIGIN must start with https:// or http://, got {public_origin:?}"));
        }

        let auth = match get("AUTH_MODE").unwrap_or("token") {
            "token" => {
                let token = get("AUTH_TOKEN").ok_or("AUTH_MODE=token needs AUTH_TOKEN (any long random string)")?;
                if token.len() < 16 {
                    return Err("AUTH_TOKEN must be at least 16 characters".into());
                }
                AuthMode::Token(token.to_owned())
            }
            "oidc" => AuthMode::Oidc(Box::new(oidc_from(&get)?)),
            other => return Err(format!("AUTH_MODE must be token or oidc, got {other:?}")),
        };

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

fn oidc_from<'a>(get: &impl Fn(&str) -> Option<&'a str>) -> Result<OidcConfig, String> {
    let need = |name: &str, provider: &str| {
        get(name).map(str::to_owned).ok_or_else(|| format!("AUTH_PROVIDERS includes {provider}, which needs {name}"))
    };
    let names = get("AUTH_PROVIDERS").ok_or("AUTH_MODE=oidc needs AUTH_PROVIDERS (google, apple or google,apple)")?;
    let mut oidc = OidcConfig::default();
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match name {
            "google" => {
                oidc.google = Some(GoogleConfig {
                    client_id: need("GOOGLE_CLIENT_ID", name)?,
                    client_secret: need("GOOGLE_CLIENT_SECRET", name)?,
                    discovery_url: GOOGLE_DISCOVERY_URL.into(),
                })
            }
            "apple" => {
                // Platforms that cannot hold newlines in a secret get "\n" written out instead.
                let pem = need("APPLE_PRIVATE_KEY", name)?.replace("\\n", "\n");
                jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes())
                    .map_err(|e| format!("APPLE_PRIVATE_KEY is not the PEM of a .p8 key: {e}"))?;
                oidc.apple = Some(AppleConfig {
                    team_id: need("APPLE_TEAM_ID", name)?,
                    key_id: need("APPLE_KEY_ID", name)?,
                    services_id: need("APPLE_SERVICES_ID", name)?,
                    private_key_pem: pem,
                    endpoints: AppleEndpoints::default(),
                })
            }
            other => return Err(format!("AUTH_PROVIDERS: unknown provider {other:?} (google, apple)")),
        }
    }
    if oidc.google.is_none() && oidc.apple.is_none() {
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
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "off"), ("AUTH_TOKEN", "0123456789abcdef")])).is_err());
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
        assert_eq!(oidc.google.as_ref().unwrap().discovery_url, GOOGLE_DISCOVERY_URL);
        assert!(oidc.apple.is_none());
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
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "oidc"), ("AUTH_PROVIDERS", "github")])).is_err());
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
    fn emails() {
        assert!(plausible_email("a@b.c"));
        assert!(!plausible_email("a b@c"));
        assert!(!plausible_email("@b"));
        assert!(!plausible_email("a@"));
        assert!(!plausible_email("a@b@c"));
    }
}
