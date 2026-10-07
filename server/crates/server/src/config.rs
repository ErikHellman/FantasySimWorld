//! Configuration, read once from the environment at start.

use std::net::SocketAddr;
use std::path::PathBuf;

use fsw_brain::settings::{ApiKey, ConversationMode, EmbedSettings, LlmSettings, ReasoningMode, WireProtocol};

use crate::guests::{DEFAULT_SEATS, GuestLimits};

/// How viewers are let in. The only mode without sign-in, [`AuthMode::Off`],
/// refuses to start anywhere but on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// No sign-in: everyone is an admin. For local development only, so it needs
    /// a loopback bind address and an `http://localhost` (or loopback IP) origin.
    Off,
    /// One shared secret, the admin's. Browsers post it to `/auth/token` once and get a
    /// cookie; everyone without it watches as a guest.
    Token(String),
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
    /// `AUTH_MODE`: `token` (with `AUTH_TOKEN`, the admin's) or `off` (everyone is an admin; local development only)
    pub auth: AuthMode,
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
    /// The embedding model, from `EMBED_BASE_URL` and `EMBED_MODEL` (see `embed_from`). Off unless both are set.
    pub embed: EmbedSettings,
    /// `EMBED_API_KEY`: the embedding endpoint's key, if it needs one (masked in `Debug`)
    pub embed_api_key: ApiKey,
    /// `MAX_VIEWERS`: how many guests may watch at once (default 25); `0` for no cap. The rest wait in line.
    pub guests: GuestLimits,
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
            other => return Err(format!("AUTH_MODE must be token or off, got {other:?}")),
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

        let seats = match get("MAX_VIEWERS") {
            None => DEFAULT_SEATS,
            Some(text) => match text.parse::<u32>() {
                Ok(seats) => seats,
                Err(_) => return Err(format!("MAX_VIEWERS must be a whole number (0 for no cap), got {text:?}")),
            },
        };

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
        let embed = embed_from(&get)?;

        Ok(Self {
            bind,
            public_origin,
            static_dir: get("STATIC_DIR").unwrap_or("dist").into(),
            data_dir: get("DATA_DIR").unwrap_or("data").into(),
            auth,
            trust_proxy,
            pretty_logs: get("LOG_FORMAT") == Some("pretty"),
            initial_speed,
            llm,
            llm_api_key: ApiKey::new(get("LLM_API_KEY").unwrap_or("")),
            embed,
            embed_api_key: ApiKey::new(get("EMBED_API_KEY").unwrap_or("")),
            guests: GuestLimits { seats },
        })
    }

    /// Cookies get `Secure` (and the `__Host-` / `__Secure-` prefixes) only when served over https.
    pub fn secure_cookies(&self) -> bool {
        self.public_origin.starts_with("https://")
    }
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

/// The embedding model. Both variables or neither, like the language model's.
fn embed_from<'a>(get: &impl Fn(&str) -> Option<&'a str>) -> Result<EmbedSettings, String> {
    let base_url = get("EMBED_BASE_URL").unwrap_or("").trim_end_matches('/').to_owned();
    let model = get("EMBED_MODEL").unwrap_or("").to_owned();
    match (base_url.is_empty(), model.is_empty()) {
        (false, true) => return Err("EMBED_BASE_URL is set but EMBED_MODEL is not; set both or neither".into()),
        (true, false) => return Err("EMBED_MODEL is set but EMBED_BASE_URL is not; set both or neither".into()),
        _ => {}
    }
    if !(base_url.is_empty() || base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(format!("EMBED_BASE_URL must start with https:// or http://, got {base_url:?}"));
    }
    Ok(EmbedSettings { base_url, model })
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
        assert_eq!(config.guests, GuestLimits { seats: 25 });
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
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "nope"), ("AUTH_TOKEN", "0123456789abcdef")])).is_err());
        assert!(Config::from_vars(vars(&[("AUTH_MODE", "oidc"), ("AUTH_TOKEN", "0123456789abcdef")])).is_err());
    }

    #[test]
    fn proxy_flag_is_checked() {
        let with =
            |name: &str, value: &str| Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef"), (name, value)]));
        assert!(with("TRUST_PROXY", "maybe").is_err());
        assert!(!with("TRUST_PROXY", "0").unwrap().trust_proxy);
    }

    #[test]
    fn max_viewers_is_a_whole_number_and_zero_means_no_cap() {
        let with = |value: &str| Config::from_vars(vars(&[("AUTH_TOKEN", "0123456789abcdef"), ("MAX_VIEWERS", value)]));
        assert_eq!(with("10").unwrap().guests.seats, 10);
        assert_eq!(with("0").unwrap().guests.seats, 0);
        for bad in ["-1", "2.5", "lots", "4294967296"] {
            assert!(with(bad).unwrap_err().contains("MAX_VIEWERS"), "{bad:?} should be refused");
        }
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
    fn embeddings_are_off_unless_both_settings_are_given() {
        let token = ("AUTH_TOKEN", "0123456789abcdef");
        let config = Config::from_vars(vars(&[token])).unwrap();
        assert!(!config.embed.enabled());
        assert!(config.embed_api_key.is_empty());

        let config = Config::from_vars(vars(&[
            token,
            ("EMBED_BASE_URL", "http://localhost:1234/v1/"),
            ("EMBED_MODEL", "nomic-embed-text"),
            ("EMBED_API_KEY", "sk-embed-1234"),
        ]))
        .unwrap();
        assert!(config.embed.enabled());
        assert_eq!(config.embed.base_url, "http://localhost:1234/v1");
        assert_eq!(config.embed.model, "nomic-embed-text");
        assert_eq!(config.embed_api_key.expose(), "sk-embed-1234");
        assert!(!format!("{config:?}").contains("sk-embed-1234"));

        let only_model = Config::from_vars(vars(&[token, ("EMBED_MODEL", "m")])).unwrap_err();
        assert!(only_model.contains("EMBED_BASE_URL"), "{only_model}");
        let only_url = Config::from_vars(vars(&[token, ("EMBED_BASE_URL", "http://x/v1")])).unwrap_err();
        assert!(only_url.contains("EMBED_MODEL"), "{only_url}");
        let bad = Config::from_vars(vars(&[token, ("EMBED_BASE_URL", "ftp://x"), ("EMBED_MODEL", "m")])).unwrap_err();
        assert!(bad.contains("EMBED_BASE_URL"), "{bad}");
    }
}
