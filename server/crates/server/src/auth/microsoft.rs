//! Microsoft (personal accounts and work or school accounts in Entra ID).
//!
//! The `email` claim of a work or school account is whatever the tenant's
//! admin typed in, so a tenant could claim anybody's address ("nOAuth"). An
//! address counts as verified only for a personal account, whose e-mail
//! Microsoft has confirmed, or when the optional `xms_edov` claim says the
//! account's tenant has proven it owns the address's domain.

use super::provider::IdClaims;

/// The tenant every personal Microsoft account lives in.
pub const CONSUMER_TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// Whether Microsoft vouches for the id_token's e-mail.
pub fn email_verified(claims: &IdClaims) -> bool {
    claims.tid.as_deref() == Some(CONSUMER_TENANT) || claims.xms_edov
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(json: &str) -> IdClaims {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn personal_accounts_and_domain_verified_tenants_count() {
        let personal = format!(r#"{{"sub":"1","iss":"i","tid":"{CONSUMER_TENANT}","email":"a@outlook.com"}}"#);
        assert!(email_verified(&claims(&personal)));
        assert!(email_verified(&claims(r#"{"sub":"1","iss":"i","tid":"t","xms_edov":true}"#)));
        assert!(!email_verified(&claims(r#"{"sub":"1","iss":"i","tid":"t","xms_edov":false}"#)));
        assert!(!email_verified(&claims(r#"{"sub":"1","iss":"i","tid":"t","email_verified":true}"#)));
        assert!(!email_verified(&claims(r#"{"sub":"1","iss":"i"}"#)));
    }
}
