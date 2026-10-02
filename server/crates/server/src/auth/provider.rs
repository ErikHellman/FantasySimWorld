//! Sign-in providers and the identity they vouch for.

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
