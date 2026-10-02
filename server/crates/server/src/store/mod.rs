//! SQLite persistence (`<DATA_DIR>/world.db`, WAL).
//!
//! One store thread owns the only write connection and takes [`StoreOp`]s from
//! a channel; every write goes through it, so writes never contend. Reads open
//! a short-lived connection inside `spawn_blocking`. Chronicle appends are
//! buffered and written in one transaction at most once a second.
//!
//! What the world task needs (see each method for details):
//!
//! - boot: [`Store::active_world`], then [`Store::load_latest_snapshot`]; with
//!   no world yet, [`Store::create_world`] (activated) and a first snapshot.
//! - every 30 s and after `save_point` events: [`Store::write_snapshot`] with
//!   the state JSON and the brain sidecar; the store picks the tier.
//! - every tick with something to keep: [`Store::record_commands`] (applied
//!   commands) and [`Store::append_chronicle`].
//! - new world / import: [`Store::create_world`] with `activate = true`, or
//!   [`Store::set_active_world`].
//! - shutdown: a `reason = Shutdown` snapshot, then [`Store::close`].
//!
//! [`maintenance::spawn`] runs retention pruning hourly and the nightly backup.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::api::wire::ChronicleEntry;

pub mod brain_log;
pub mod chronicle;
pub mod commands;
pub mod db;
pub mod maintenance;
pub mod migrations;
pub mod settings;
pub mod snapshots;
pub mod users;
pub mod worlds;

pub use brain_log::{BrainLogWrite, UsageRow};
pub use commands::CommandRecord;
pub use settings::ApiKey;
pub use snapshots::{LoadedSnapshot, SnapshotReason, SnapshotSaved, SnapshotWrite, Tier};
pub use users::{LoginOutcome, SessionRecord, User};
pub use worlds::World;

pub const DB_FILE: &str = "world.db";
/// Chronicle appends wait at most this long before they are written.
pub const CHRONICLE_FLUSH: Duration = Duration::from_secs(1);
const QUEUE: usize = 1024;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    /// the store thread has stopped (shutdown) or a write panicked
    Closed,
    NotFound(&'static str),
    Corrupt(String),
    /// a request the store will not carry out (e.g. reading the API key as a plain setting)
    Refused(&'static str),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::Closed => f.write_str("the store has stopped"),
            Self::NotFound(what) => write!(f, "no such {what}"),
            Self::Corrupt(why) => write!(f, "corrupt data: {why}"),
            Self::Refused(why) => write!(f, "refused: {why}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

type WriteFn = Box<dyn FnOnce(&mut Connection) + Send>;

/// Work for the store thread.
enum StoreOp {
    /// Runs on the write connection; the closure sends its own reply.
    Write(WriteFn),
    /// Buffered and written within [`CHRONICLE_FLUSH`].
    Chronicle { world_id: i64, entries: Vec<ChronicleEntry> },
    /// Writes buffered chronicle entries now.
    Flush(oneshot::Sender<()>),
    /// Flushes, checkpoints the WAL and stops the thread.
    Close(oneshot::Sender<()>),
}

/// Cheap to clone; every clone talks to the same store thread.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    data_dir: PathBuf,
    ops: mpsc::Sender<StoreOp>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Store {
    /// Opens (or creates) `<data_dir>/world.db`, migrates it and starts the store thread.
    pub async fn open(data_dir: &Path) -> Result<Self> {
        let data_dir = data_dir.to_path_buf();
        let path = data_dir.join(DB_FILE);
        let (conn, path) = tokio::task::spawn_blocking(move || -> Result<_> {
            std::fs::create_dir_all(path.parent().expect("the db file has a directory"))?;
            let mut conn = db::open_writer(&path)?;
            migrations::run(&mut conn)?;
            Ok((conn, path))
        })
        .await
        .map_err(|_| StoreError::Closed)??;
        let (ops, rx) = mpsc::channel(QUEUE);
        let thread = std::thread::Builder::new().name("store".into()).spawn(move || run_writer(conn, rx))?;
        Ok(Self { inner: Arc::new(Inner { path, data_dir, ops, thread: Mutex::new(Some(thread)) }) })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    /// Runs `f` on the write connection and waits for its result.
    pub(crate) async fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        let op: WriteFn = Box::new(move |conn| {
            let _ = tx.send(f(conn));
        });
        self.inner.ops.send(StoreOp::Write(op)).await.map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }

    /// Runs `f` on a fresh read-only connection on the blocking pool.
    pub(crate) async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let path = self.inner.path.clone();
        tokio::task::spawn_blocking(move || f(&db::open_reader(&path)?)).await.map_err(|_| StoreError::Closed)?
    }

    /// Queues chronicle entries; they reach the database within [`CHRONICLE_FLUSH`]
    /// (or on [`Store::flush`] / [`Store::close`]), in order.
    pub async fn append_chronicle(&self, world_id: i64, entries: Vec<ChronicleEntry>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        self.inner.ops.send(StoreOp::Chronicle { world_id, entries }).await.map_err(|_| StoreError::Closed)
    }

    /// Writes everything still buffered.
    pub async fn flush(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.inner.ops.send(StoreOp::Flush(tx)).await.map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)
    }

    /// Flushes, checkpoints the WAL into the main file and stops the store thread.
    /// Later calls on any clone fail with [`StoreError::Closed`].
    pub async fn close(&self) {
        let (tx, rx) = oneshot::channel();
        if self.inner.ops.send(StoreOp::Close(tx)).await.is_ok() {
            let _ = rx.await;
        }
        let thread = self.inner.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = thread {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

/// The store thread. A small current-thread runtime gives it a timer for the
/// chronicle flush; all database work is plain blocking calls.
fn run_writer(mut conn: Connection, mut rx: mpsc::Receiver<StoreOp>) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("store runtime starts");
    runtime.block_on(async move {
        let mut pending: Vec<(i64, ChronicleEntry)> = Vec::new();
        let mut flush_at: Option<tokio::time::Instant> = None;
        loop {
            let op = match flush_at {
                Some(at) => tokio::select! {
                    op = rx.recv() => op,
                    () = tokio::time::sleep_until(at) => {
                        flush_chronicle(&mut conn, &mut pending);
                        flush_at = None;
                        continue;
                    }
                },
                None => rx.recv().await,
            };
            let Some(op) = op else { break };
            match op {
                StoreOp::Write(f) => {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut conn)));
                    if result.is_err() {
                        tracing::error!("a store write panicked");
                    }
                }
                StoreOp::Chronicle { world_id, entries } => {
                    pending.extend(entries.into_iter().map(|e| (world_id, e)));
                    flush_at.get_or_insert_with(|| tokio::time::Instant::now() + CHRONICLE_FLUSH);
                }
                StoreOp::Flush(reply) => {
                    flush_chronicle(&mut conn, &mut pending);
                    flush_at = None;
                    let _ = reply.send(());
                }
                StoreOp::Close(reply) => {
                    flush_chronicle(&mut conn, &mut pending);
                    checkpoint(&conn);
                    let _ = reply.send(());
                    return;
                }
            }
        }
        // Every handle dropped without close(): still leave a clean file.
        flush_chronicle(&mut conn, &mut pending);
        checkpoint(&conn);
    });
}

fn flush_chronicle(conn: &mut Connection, pending: &mut Vec<(i64, ChronicleEntry)>) {
    if pending.is_empty() {
        return;
    }
    if let Err(error) = chronicle::insert(conn, pending) {
        // Kept for the next attempt; a broken database will show up elsewhere too.
        tracing::error!(%error, entries = pending.len(), "writing the chronicle failed");
        return;
    }
    pending.clear();
}

/// Folds the WAL into the main file at exit. Litestream keeps a read lock on the
/// database, so a full checkpoint would only wait out the busy timeout: try it
/// without waiting, then copy what can be copied and leave the rest to Litestream.
fn checkpoint(conn: &Connection) {
    let _ = conn.busy_timeout(Duration::ZERO);
    match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0)) {
        Ok(0) => tracing::info!("checkpointed the WAL"),
        Ok(_) => match conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| r.get::<_, i64>(0)) {
            Ok(_) => tracing::info!("checkpointed the WAL as far as a reader (Litestream) allows"),
            Err(error) => tracing::error!(%error, "WAL checkpoint failed"),
        },
        Err(error) => tracing::error!(%error, "WAL checkpoint failed"),
    }
    let _ = conn.execute_batch("PRAGMA optimize");
}

/// Wall clock in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub const DAY_MS: i64 = 24 * 3600 * 1000;

/// Days since the epoch in UTC (a new one starts at 00:00 UTC).
pub fn utc_day(ms: i64) -> i64 {
    ms.div_euclid(DAY_MS)
}

/// A unit-like enum's serde name ("save_point", "admin", …), for TEXT columns.
pub(crate) fn enum_text<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("only used with unit-variant enums"),
    }
}

/// The reverse of [`enum_text`].
pub(crate) fn enum_parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(|_| StoreError::Corrupt(text.into()))
}

pub(crate) fn zstd_compress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::bulk::compress(bytes, 3)?)
}

pub(crate) fn zstd_decompress(bytes: &[u8], size_hint: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(size_hint);
    zstd::stream::copy_decode(bytes, &mut out)?;
    Ok(out)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A fresh store in a temporary directory (kept alive by the returned guard).
    pub async fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Store::open(dir.path()).await.unwrap(), dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::wire::ChronicleKind;

    fn entry(tick: u64, text: &str) -> ChronicleEntry {
        ChronicleEntry { tick, kind: ChronicleKind::World, text: text.into(), ids: vec![] }
    }

    #[tokio::test]
    async fn chronicle_is_batched_and_flushed_on_close() {
        let (store, dir) = testing::store().await;
        let world = store.create_world("Vale", 7, true).await.unwrap();
        store.append_chronicle(world.id, vec![entry(1, "a")]).await.unwrap();
        // Not written yet: the batch waits for up to a second.
        let page = store.chronicle_page(world.id, None, 10).await.unwrap();
        assert!(page.entries.is_empty());
        store.append_chronicle(world.id, vec![entry(2, "b")]).await.unwrap();
        store.close().await;
        assert!(matches!(store.flush().await, Err(StoreError::Closed)));

        // The WAL was checkpointed into the main file.
        let wal = dir.path().join(format!("{DB_FILE}-wal"));
        assert!(std::fs::metadata(&wal).map_or(0, |m| m.len()) == 0);
        let store = Store::open(dir.path()).await.unwrap();
        let page = store.chronicle_page(world.id, None, 10).await.unwrap();
        assert_eq!(page.entries.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[tokio::test]
    async fn chronicle_flushes_within_a_second() {
        let (store, _dir) = testing::store().await;
        let world = store.create_world("Vale", 7, true).await.unwrap();
        store.append_chronicle(world.id, vec![entry(1, "a")]).await.unwrap();
        tokio::time::sleep(CHRONICLE_FLUSH + Duration::from_millis(300)).await;
        assert_eq!(store.chronicle_page(world.id, None, 10).await.unwrap().entries.len(), 1);
    }

    #[test]
    fn utc_days_start_at_midnight() {
        assert_eq!(utc_day(0), 0);
        assert_eq!(utc_day(DAY_MS - 1), 0);
        assert_eq!(utc_day(DAY_MS), 1);
        assert_eq!(utc_day(-1), -1);
    }
}
