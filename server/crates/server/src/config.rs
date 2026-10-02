//! Configuration, read once from the environment at start.

use std::net::SocketAddr;
use std::path::PathBuf;

/// How viewers are let in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// One shared secret, for local work and the first deployments. Browsers
    /// open `/auth/token?token=…` once and get a cookie.
    Token(String),
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
    /// `AUTH_MODE` (only `token` so far) with `AUTH_TOKEN`
    pub auth: AuthMode,
    /// `LOG_FORMAT=pretty` for humans; JSON lines otherwise
    pub pretty_logs: bool,
    /// `INITIAL_SPEED`: 0, 1 (default), 2 or 4. 0 starts paused, which leaves only
    /// keep-alives on the stream (for checking a proxy's idle timeout).
    pub initial_speed: u8,
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
            other => return Err(format!("AUTH_MODE {other:?} is not supported yet (use token)")),
        };

        let initial_speed = match get("INITIAL_SPEED") {
            None => 1,
            Some(text) => match text.parse::<u8>() {
                Ok(speed) if crate::world_task::SPEEDS.contains(&speed) => speed,
                _ => return Err(format!("INITIAL_SPEED must be 0, 1, 2 or 4, got {text:?}")),
            },
        };

        Ok(Self {
            bind,
            public_origin,
            static_dir: get("STATIC_DIR").unwrap_or("dist").into(),
            data_dir: get("DATA_DIR").unwrap_or("data").into(),
            auth,
            pretty_logs: get("LOG_FORMAT") == Some("pretty"),
            initial_speed,
        })
    }

    /// Cookies get `Secure` (and the `__Host-` prefix) only when served over https.
    pub fn secure_cookies(&self) -> bool {
        self.public_origin.starts_with("https://")
    }
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
    }
}
