//! The shared-world server. See `docs/superpowers/specs/2026-10-02-backend-migration-plan.md`.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use tokio_util::sync::CancellationToken;

pub mod api;
pub mod auth;
pub mod brain_task;
pub mod chat;
pub mod config;
pub mod digest;
pub mod encode;
pub mod guests;
pub mod store;
pub mod world_task;

use auth::Auth;
use config::Config;
use fsw_brain::brain::Brain;
use store::Store;
use world_task::World;

/// Shared by every request handler.
pub struct AppState {
    pub config: Config,
    /// hash of the static world layout (`fsw_world::navgrid::world_hash`)
    pub world_hash: String,
    pub world: Arc<World>,
    pub viewers: AtomicU32,
    pub backlog_drops: AtomicU32,
    pub store: Store,
    pub auth: Auth,
    /// the language-model brain (off unless `LLM_BASE_URL` and `LLM_MODEL` are set)
    pub brain: Brain,
    /// the viewers' chat (in memory: survives a new world, not a restart)
    pub chat: Arc<chat::ChatHub>,
    /// the viewers' seats and the line for them (in memory)
    pub guests: Arc<guests::GuestHub>,
    /// cancelled on SIGTERM: streams end so the server can stop
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new(
        config: Config,
        world: Arc<World>,
        store: Store,
        brain: Brain,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        let guests = Arc::new(guests::GuestHub::new(config.guests));
        Arc::new(Self {
            config,
            world_hash: fsw_world::navgrid::world_hash(),
            world,
            viewers: AtomicU32::new(0),
            backlog_drops: AtomicU32::new(0),
            store,
            auth: Auth::default(),
            brain,
            chat: Arc::default(),
            guests,
            shutdown,
        })
    }
}

pub use api::router::router;
