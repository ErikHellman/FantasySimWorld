//! The schema, as numbered migrations applied once each, in order.
//!
//! Times are milliseconds since the Unix epoch. Never edit a migration that
//! has shipped; add the next number instead.

use rusqlite::Connection;

use super::{Result, StoreError, now_ms};

const MIGRATIONS: &[(i64, &str)] = &[(1, V1), (2, V2), (3, V3)];

const V1: &str = "
CREATE TABLE worlds (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    seed INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1))
) STRICT;
CREATE UNIQUE INDEX worlds_one_active ON worlds (is_active) WHERE is_active = 1;

CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY,
    world_id INTEGER NOT NULL REFERENCES worlds (id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    game_day INTEGER NOT NULL,
    reason TEXT NOT NULL,
    tier INTEGER NOT NULL CHECK (tier BETWEEN 0 AND 3),
    save_version INTEGER NOT NULL,
    encoding TEXT NOT NULL,
    size_raw INTEGER NOT NULL,
    body BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX snapshots_by_world ON snapshots (world_id, id);

CREATE TABLE commands (
    id INTEGER PRIMARY KEY,
    world_id INTEGER NOT NULL REFERENCES worlds (id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    source TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX commands_by_tick ON commands (world_id, tick, seq);
CREATE INDEX commands_by_age ON commands (created_at);

CREATE TABLE chronicle (
    id INTEGER PRIMARY KEY,
    world_id INTEGER NOT NULL REFERENCES worlds (id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    ids TEXT NOT NULL
) STRICT;
CREATE INDEX chronicle_by_tick ON chronicle (world_id, tick, id);

CREATE TABLE brain_log (
    id INTEGER PRIMARY KEY,
    world_id INTEGER NOT NULL REFERENCES worlds (id) ON DELETE CASCADE,
    tick INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    character_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    latency_ms INTEGER NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    summary TEXT NOT NULL,
    prompt BLOB,
    response BLOB,
    commands TEXT NOT NULL
) STRICT;
CREATE INDEX brain_log_by_age ON brain_log (created_at);

CREATE TABLE brain_usage (
    day INTEGER NOT NULL,
    kind TEXT NOT NULL,
    requests INTEGER NOT NULL DEFAULT 0,
    failures INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, kind)
) STRICT;

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'viewer')),
    created_at INTEGER NOT NULL,
    last_login_at INTEGER
) STRICT;

CREATE TABLE identities (
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    email_at_link TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (provider, subject)
) STRICT;
CREATE INDEX identities_by_user ON identities (user_id);

CREATE TABLE allowlist (
    email TEXT PRIMARY KEY,
    role TEXT NOT NULL CHECK (role IN ('admin', 'viewer')),
    added_by TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    note TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE sessions (
    id BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    user_agent TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE INDEX sessions_by_user ON sessions (user_id);
CREATE INDEX sessions_by_expiry ON sessions (expires_at);

CREATE TABLE auth_attempts (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL,
    email TEXT NOT NULL,
    outcome TEXT NOT NULL,
    at INTEGER NOT NULL
) STRICT;
CREATE INDEX auth_attempts_by_time ON auth_attempts (at);
";

/// The language model moved to the server's environment: forget what the admin panel
/// saved, so the API key no longer sits in the database (or in its backups).
const V2: &str = "DELETE FROM settings WHERE key IN ('llm', 'llm.api_key');";

/// Viewers' credits: a balance per account, and a record of what was bought. `power_name` is
/// the (lower-cased) name an account last spent under, which is how the villagers know it.
const V3: &str = "
CREATE TABLE credits (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    balance INTEGER NOT NULL,
    earned_total INTEGER NOT NULL,
    spent_total INTEGER NOT NULL,
    power_name TEXT,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE INDEX credits_by_power ON credits (power_name);

CREATE TABLE credit_purchases (
    id INTEGER PRIMARY KEY,
    user_id INTEGER,
    name TEXT NOT NULL,
    world_id INTEGER NOT NULL,
    offer TEXT NOT NULL,
    cost INTEGER NOT NULL,
    tick INTEGER NOT NULL,
    at INTEGER NOT NULL
) STRICT;
CREATE INDEX credit_purchases_by_time ON credit_purchases (at);
";

/// Brings the schema up to date. Refuses a database written by a newer server.
pub fn run(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL) STRICT",
    )?;
    let current: i64 = conn.query_row("SELECT coalesce(max(version), 0) FROM schema_migrations", [], |r| r.get(0))?;
    let latest = MIGRATIONS.last().map_or(0, |(v, _)| *v);
    if current > latest {
        return Err(StoreError::Corrupt(format!("database schema {current} is newer than this server ({latest})")));
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)", (version, now_ms()))?;
        tx.commit()?;
        tracing::info!(version, "applied database migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = super::super::db::open_writer(&dir.path().join("t.db")).unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        let versions: i64 = conn.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(versions, MIGRATIONS.len() as i64);
        // auto_vacuum survived table creation
        let mode: i64 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, 2);
    }

    #[test]
    fn forgets_the_model_settings_the_admin_panel_saved() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = super::super::db::open_writer(&dir.path().join("t.db")).unwrap();
        run(&mut conn).unwrap();
        for key in ["llm", "llm.api_key", "llm.capabilities"] {
            conn.execute("INSERT INTO settings (key, value, updated_at) VALUES (?1, '\"x\"', 0)", [key]).unwrap();
        }
        // As it ran on a database from before it (later migrations cannot run twice).
        conn.execute_batch(V2).unwrap();
        let mut keys = conn.prepare("SELECT key FROM settings ORDER BY key").unwrap();
        let keys: Vec<String> = keys.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(keys, ["llm.capabilities"]);
    }

    #[test]
    fn one_active_world_at_most() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = super::super::db::open_writer(&dir.path().join("t.db")).unwrap();
        run(&mut conn).unwrap();
        let add = "INSERT INTO worlds (name, seed, created_at, is_active) VALUES ('w', 1, 0, ?1)";
        conn.execute(add, [1]).unwrap();
        conn.execute(add, [0]).unwrap();
        assert!(conn.execute(add, [1]).is_err());
    }

    #[test]
    fn refuses_a_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = super::super::db::open_writer(&dir.path().join("t.db")).unwrap();
        run(&mut conn).unwrap();
        conn.execute("INSERT INTO schema_migrations VALUES (999, 0)", []).unwrap();
        assert!(matches!(run(&mut conn), Err(StoreError::Corrupt(_))));
    }
}
