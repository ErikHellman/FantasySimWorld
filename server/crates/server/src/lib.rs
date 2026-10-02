//! The shared-world server. See `docs/superpowers/specs/2026-10-02-backend-migration-plan.md`.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;

pub mod api;
pub mod auth;
pub mod config;
pub mod store;
pub mod world_task;

use config::Config;
use world_task::{FrameBatch, WorldClock, WorldHandles};

/// Shared by every request handler.
pub struct AppState {
    pub config: Config,
    /// random per start; prefixes stream event ids
    pub boot_id: String,
    /// hash of the static world layout (filled in once the world is ported, phase 1)
    pub world_hash: String,
    pub clock: watch::Receiver<WorldClock>,
    pub frames: broadcast::Sender<Arc<FrameBatch>>,
    pub viewers: AtomicU32,
    pub backlog_drops: AtomicU32,
    /// cancelled on SIGTERM: streams end so the server can stop
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new(config: Config, world: WorldHandles, shutdown: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            config,
            boot_id: new_boot_id(),
            world_hash: "unported".into(),
            clock: world.clock,
            frames: world.frames,
            viewers: AtomicU32::new(0),
            backlog_drops: AtomicU32::new(0),
            shutdown,
        })
    }
}

fn new_boot_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub use api::router::router;
