//! People: users, their sign-in identities, the allowlist, sessions and refused attempts.
//!
//! A user exists only while their e-mail is on the allowlist: removing it
//! deletes the user, and with them their identities and sessions.

use rusqlite::{Connection, OptionalExtension, Row};

use super::{Result, Store, enum_parse, enum_text, now_ms};
use crate::api::wire::{AllowlistEntry, AuthAttempt, Role};
use crate::auth::provider::Identity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    /// the allowlisted e-mail they were let in under (lowercase)
    pub email: String,
    pub display_name: String,
    pub role: Role,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    /// a known identity
    Existing(User),
    /// an allowlisted e-mail signing in with this identity for the first time
    Linked(User),
    /// recorded in `auth_attempts` with this outcome
    Refused { outcome: &'static str, email: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub user: User,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_seen_at: i64,
}

/// Lowercased and trimmed; what every e-mail column holds.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

const USER_COLUMNS: &str = "u.id, u.email, u.display_name, u.role, u.created_at, u.last_login_at";

fn user(row: &Row) -> rusqlite::Result<(i64, String, String, String, i64, Option<i64>)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn to_user(
    (id, email, display_name, role, created_at, last_login_at): (i64, String, String, String, i64, Option<i64>),
) -> Result<User> {
    Ok(User { id, email, display_name, role: enum_parse(&role)?, created_at, last_login_at })
}

fn user_by(conn: &Connection, filter: &str, param: impl rusqlite::ToSql) -> Result<Option<User>> {
    conn.query_row(&format!("SELECT {USER_COLUMNS} FROM users u {filter}"), [param], user)
        .optional()?
        .map(to_user)
        .transpose()
}

fn record_attempt(conn: &Connection, provider: &str, email: &str, outcome: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO auth_attempts (provider, email, outcome, at) VALUES (?1, ?2, ?3, ?4)",
        (provider, email, outcome, now_ms()),
    )?;
    Ok(())
}

/// identities → allowlist (create the user, link the identity) → refuse and record.
fn resolve(conn: &mut Connection, identity: &Identity) -> Result<LoginOutcome> {
    let now = now_ms();
    let provider = identity.provider.as_str();
    let tx = conn.transaction()?;
    let known = tx
        .query_row(
            &format!(
                "SELECT {USER_COLUMNS} FROM users u JOIN identities i ON i.user_id = u.id
                 WHERE i.provider = ?1 AND i.subject = ?2"
            ),
            (provider, &identity.subject),
            user,
        )
        .optional()?
        .map(to_user)
        .transpose()?;
    if let Some(mut found) = known {
        tx.execute("UPDATE users SET last_login_at = ?1 WHERE id = ?2", (now, found.id))?;
        tx.commit()?;
        found.last_login_at = Some(now);
        return Ok(LoginOutcome::Existing(found));
    }

    let email = identity.email.as_deref().map(normalize_email).unwrap_or_default();
    let refused = |tx: rusqlite::Transaction, outcome: &'static str| -> Result<LoginOutcome> {
        record_attempt(&tx, provider, &email, outcome)?;
        tx.commit()?;
        Ok(LoginOutcome::Refused { outcome, email: email.clone() })
    };
    if email.is_empty() {
        return refused(tx, "no_email");
    }
    if !identity.email_verified {
        return refused(tx, "email_unverified");
    }
    let role: Option<String> =
        tx.query_row("SELECT role FROM allowlist WHERE email = ?1", [&email], |r| r.get(0)).optional()?;
    let Some(role) = role else {
        return refused(tx, "not_allowlisted");
    };
    let display_name = identity
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| email.split('@').next().unwrap_or_default().to_owned(), str::to_owned);
    // The user may exist already (signed in before with another provider).
    tx.execute(
        "INSERT INTO users (email, display_name, role, created_at, last_login_at) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (email) DO UPDATE SET role = excluded.role, last_login_at = excluded.last_login_at",
        (&email, &display_name, &role, now),
    )?;
    let linked = user_by(&tx, "WHERE u.email = ?1", &email)?.expect("the user was just written");
    tx.execute(
        "INSERT INTO identities (provider, subject, user_id, email_at_link, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        (provider, &identity.subject, linked.id, &email, now),
    )?;
    tx.commit()?;
    Ok(LoginOutcome::Linked(linked))
}

fn allowlist_entry(row: &Row) -> rusqlite::Result<(String, String, String, i64, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
}

fn to_entry((email, role, added_by, added_at, note): (String, String, String, i64, String)) -> Result<AllowlistEntry> {
    Ok(AllowlistEntry { email, role: enum_parse(&role)?, added_by, added_at: added_at as u64, note })
}

fn allow(conn: &mut Connection, email: &str, role: Role, added_by: &str, note: &str) -> Result<AllowlistEntry> {
    let email = normalize_email(email);
    let role = enum_text(&role);
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO allowlist (email, role, added_by, added_at, note) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (email) DO UPDATE SET role = excluded.role, note = excluded.note",
        (&email, &role, added_by, now_ms(), note),
    )?;
    tx.execute("UPDATE users SET role = ?1 WHERE email = ?2", (&role, &email))?;
    let entry = tx.query_row(
        "SELECT email, role, added_by, added_at, note FROM allowlist WHERE email = ?1",
        [&email],
        allowlist_entry,
    )?;
    tx.commit()?;
    to_entry(entry)
}

impl Store {
    /// Decides who a verified identity is, creating the user on first sign-in.
    pub async fn resolve_login(&self, identity: Identity) -> Result<LoginOutcome> {
        self.write(move |conn| resolve(conn, &identity)).await
    }

    pub async fn user_by_email(&self, email: &str) -> Result<Option<User>> {
        let email = normalize_email(email);
        self.read(move |conn| user_by(conn, "WHERE u.email = ?1", &email)).await
    }

    /// Stores a new session under the token's hash.
    pub async fn create_session(&self, id: [u8; 32], user_id: i64, user_agent: &str, expires_at: i64) -> Result<()> {
        let user_agent: String = user_agent.chars().take(300).collect();
        self.write(move |conn| {
            let now = now_ms();
            conn.execute(
                "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at, user_agent)
                 VALUES (?1, ?2, ?3, ?4, ?3, ?5)",
                (&id[..], user_id, now, expires_at, &user_agent),
            )?;
            Ok(())
        })
        .await
    }

    /// The session and its user, unless it is unknown or expired.
    pub async fn session(&self, id: [u8; 32]) -> Result<Option<SessionRecord>> {
        self.read(move |conn| {
            let row = conn
                .query_row(
                    &format!(
                        "SELECT {USER_COLUMNS}, s.created_at, s.expires_at, s.last_seen_at
                         FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.id = ?1 AND s.expires_at > ?2"
                    ),
                    (&id[..], now_ms()),
                    |r| Ok((user(r)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?)),
                )
                .optional()?;
            row.map(|(u, created_at, expires_at, last_seen_at)| {
                Ok(SessionRecord { user: to_user(u)?, created_at, expires_at, last_seen_at })
            })
            .transpose()
        })
        .await
    }

    /// Marks the session seen now and moves its expiry (sliding window).
    pub async fn touch_session(&self, id: [u8; 32], expires_at: i64) -> Result<()> {
        self.write(move |conn| {
            conn.execute(
                "UPDATE sessions SET last_seen_at = ?1, expires_at = ?2 WHERE id = ?3",
                (now_ms(), expires_at, &id[..]),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_session(&self, id: [u8; 32]) -> Result<()> {
        self.write(move |conn| {
            conn.execute("DELETE FROM sessions WHERE id = ?1", [&id[..]])?;
            Ok(())
        })
        .await
    }

    pub async fn allowlist(&self) -> Result<Vec<AllowlistEntry>> {
        self.read(|conn| {
            let mut stmt =
                conn.prepare("SELECT email, role, added_by, added_at, note FROM allowlist ORDER BY email")?;
            let rows = stmt.query_map([], allowlist_entry)?;
            rows.map(|r| to_entry(r?)).collect()
        })
        .await
    }

    /// Adds an e-mail (or changes its role and note); a user already let in gets the new role.
    pub async fn allow(&self, email: &str, role: Role, added_by: &str, note: &str) -> Result<AllowlistEntry> {
        let (email, added_by, note) = (email.to_owned(), added_by.to_owned(), note.to_owned());
        self.write(move |conn| allow(conn, &email, role, &added_by, &note)).await
    }

    /// Removes an e-mail and its user, which ends their sessions at once. False when it was not listed.
    pub async fn disallow(&self, email: &str) -> Result<bool> {
        let email = normalize_email(email);
        self.write(move |conn| {
            let tx = conn.transaction()?;
            let removed = tx.execute("DELETE FROM allowlist WHERE email = ?1", [&email])? > 0;
            let users = tx.execute("DELETE FROM users WHERE email = ?1", [&email])?;
            tx.commit()?;
            if users > 0 {
                tracing::info!(event = "user_removed", email, "removed from the allowlist; sessions revoked");
            }
            Ok(removed)
        })
        .await
    }

    /// `ADMIN_EMAILS` at boot: each becomes (or stays) an admin. Idempotent.
    pub async fn seed_admins(&self, emails: &[String]) -> Result<()> {
        for email in emails {
            let email = normalize_email(email);
            self.write(move |conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "INSERT INTO allowlist (email, role, added_by, added_at, note) VALUES (?1, 'admin', 'ADMIN_EMAILS', ?2, '')
                     ON CONFLICT (email) DO UPDATE SET role = 'admin'",
                    (&email, now_ms()),
                )?;
                tx.execute("UPDATE users SET role = 'admin' WHERE email = ?1", [&email])?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    /// Refused sign-ins, newest first.
    pub async fn attempts(&self, limit: u32) -> Result<Vec<AuthAttempt>> {
        self.read(move |conn| {
            let mut stmt =
                conn.prepare("SELECT id, provider, email, outcome, at FROM auth_attempts ORDER BY id DESC LIMIT ?1")?;
            let rows = stmt.query_map([i64::from(limit.clamp(1, 1000))], |r| {
                Ok(AuthAttempt {
                    id: r.get::<_, i64>(0)? as u64,
                    provider: r.get(1)?,
                    email: r.get(2)?,
                    outcome: r.get(3)?,
                    at: r.get::<_, i64>(4)? as u64,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;
    use crate::auth::provider::ProviderKind;

    fn identity(subject: &str, email: &str, verified: bool) -> Identity {
        Identity {
            provider: ProviderKind::Google,
            subject: subject.into(),
            email: Some(email.into()),
            email_verified: verified,
            name: Some("Ada Lovelace".into()),
        }
    }

    #[tokio::test]
    async fn identities_then_allowlist_then_refusal() {
        let (store, _dir) = testing::store().await;
        let refused = store.resolve_login(identity("s1", "Ada@Example.com", true)).await.unwrap();
        assert_eq!(refused, LoginOutcome::Refused { outcome: "not_allowlisted", email: "ada@example.com".into() });
        let attempts = store.attempts(10).await.unwrap();
        assert_eq!((attempts[0].email.as_str(), attempts[0].outcome.as_str()), ("ada@example.com", "not_allowlisted"));

        store.allow("ADA@example.com", Role::Viewer, "admin@example.com", "").await.unwrap();
        let unverified = store.resolve_login(identity("s1", "ada@example.com", false)).await.unwrap();
        assert!(matches!(unverified, LoginOutcome::Refused { outcome: "email_unverified", .. }));

        let LoginOutcome::Linked(user) = store.resolve_login(identity("s1", "ada@example.com", true)).await.unwrap()
        else {
            panic!("allowlisted")
        };
        assert_eq!(
            (user.email.as_str(), user.display_name.as_str(), user.role),
            ("ada@example.com", "Ada Lovelace", Role::Viewer)
        );
        // The identity is linked: a later sign-in finds the user even if the e-mail claim changed.
        let again = store.resolve_login(identity("s1", "other@example.com", false)).await.unwrap();
        assert!(matches!(again, LoginOutcome::Existing(u) if u.id == user.id));

        // A second provider for the same e-mail links to the same user.
        let mut apple = identity("apple-1", "ada@example.com", true);
        apple.provider = ProviderKind::Apple;
        assert!(matches!(store.resolve_login(apple).await.unwrap(), LoginOutcome::Linked(u) if u.id == user.id));
    }

    #[tokio::test]
    async fn sessions_live_and_die_with_the_allowlist() {
        let (store, _dir) = testing::store().await;
        store.seed_admins(&["boss@example.com".into()]).await.unwrap();
        store.seed_admins(&["boss@example.com".into()]).await.unwrap();
        let entries = store.allowlist().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].role, entries[0].added_by.as_str()), (Role::Admin, "ADMIN_EMAILS"));

        let LoginOutcome::Linked(user) = store.resolve_login(identity("s1", "boss@example.com", true)).await.unwrap()
        else {
            panic!("seeded")
        };
        let far = now_ms() + 1_000_000;
        store.create_session([1; 32], user.id, "test agent", far).await.unwrap();
        store.create_session([2; 32], user.id, "test agent", now_ms() - 1).await.unwrap();
        assert_eq!(store.session([1; 32]).await.unwrap().unwrap().user.role, Role::Admin);
        assert_eq!(store.session([2; 32]).await.unwrap(), None, "expired");
        store.touch_session([1; 32], far + 5).await.unwrap();
        assert_eq!(store.session([1; 32]).await.unwrap().unwrap().expires_at, far + 5);

        // A role change reaches the signed-in user.
        store.allow("boss@example.com", Role::Viewer, "x", "demoted").await.unwrap();
        assert_eq!(store.session([1; 32]).await.unwrap().unwrap().user.role, Role::Viewer);

        assert!(store.disallow("Boss@Example.com").await.unwrap());
        assert_eq!(store.session([1; 32]).await.unwrap(), None);
        assert!(store.user_by_email("boss@example.com").await.unwrap().is_none());
        assert!(!store.disallow("boss@example.com").await.unwrap());
        let back = store.resolve_login(identity("s1", "boss@example.com", true)).await.unwrap();
        assert!(matches!(back, LoginOutcome::Refused { .. }), "the identity went with the user");
    }
}
