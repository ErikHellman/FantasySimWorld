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

use auth::Auth;
use config::Config;
use store::Store;
use world_task::{FrameBatch, WorldClock, WorldHandles};

/// Shared by every request handler.
pub struct AppState {
    pub config: Config,
    /// random per start; prefixes stream event ids
    pub boot_id: String,
    /// hash of the static world layout (`fsw_world::navgrid::world_hash`)
    pub world_hash: String,
    pub clock: watch::Receiver<WorldClock>,
    pub frames: broadcast::Sender<Arc<FrameBatch>>,
    pub viewers: AtomicU32,
    pub backlog_drops: AtomicU32,
    pub store: Store,
    pub auth: Auth,
    /// cancelled on SIGTERM: streams end so the server can stop
    pub shutdown: CancellationToken,
}

impl AppState {
    /// Fails only when sign-in cannot be set up (an unusable Apple key, no TLS backend).
    pub fn new(
        config: Config,
        world: WorldHandles,
        store: Store,
        shutdown: CancellationToken,
    ) -> Result<Arc<Self>, String> {
        let auth = Auth::new(&config)?;
        Ok(Arc::new(Self {
            config,
            boot_id: new_boot_id(),
            world_hash: fsw_world::navgrid::world_hash(),
            clock: world.clock,
            frames: world.frames,
            viewers: AtomicU32::new(0),
            backlog_drops: AtomicU32::new(0),
            store,
            auth,
            shutdown,
        }))
    }
}

fn new_boot_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub use api::router::router;
