use std::net::SocketAddr;
use std::process::ExitCode;

use fsw_server::config::{AuthMode, Config};
use fsw_server::store::{self, Store};
use fsw_server::{AppState, brain_task, chat, digest, router, world_task};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("configuration: {message}");
            return ExitCode::FAILURE;
        }
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if config.pretty_logs {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().json().with_env_filter(filter).init();
    }

    let store = match Store::open(&config.data_dir).await {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, data_dir = %config.data_dir.display(), "cannot open the database");
            return ExitCode::FAILURE;
        }
    };
    if config.auth == AuthMode::Off {
        tracing::warn!(bind = %config.bind, "AUTH_MODE=off: anyone who can reach this server is an admin (local development only)");
    }
    if config.llm.enabled {
        let llm = &config.llm;
        tracing::info!(base_url = %llm.base_url, model = %llm.model, key = !config.llm_api_key.is_empty(), "language model on");
    } else {
        tracing::info!(
            "language model off (set LLM_BASE_URL and LLM_MODEL to turn it on); villagers follow their routines"
        );
    }
    if config.embed.enabled() {
        let embed = &config.embed;
        if config.llm.enabled {
            tracing::info!(base_url = %embed.base_url, model = %embed.model, key = !config.embed_api_key.is_empty(), "memory by meaning on");
        } else {
            tracing::info!(model = %embed.model, "memory by meaning waits for the language model; nothing is embedded while it is off");
        }
    }

    let shutdown = CancellationToken::new();
    let (world, world_task) = match world_task::start(store.clone(), config.initial_speed, shutdown.clone()).await {
        Ok(started) => started,
        Err(error) => {
            tracing::error!(%error, "cannot start the world");
            store.close().await;
            return ExitCode::FAILURE;
        }
    };
    let maintenance = store::maintenance::spawn(store.clone(), shutdown.clone());
    let brain = brain_task::start(world.clone(), store.clone(), config.llm.clone(), config.llm_api_key.clone()).await;
    // Only prompts recall by meaning, so without the language model the indexer would embed memories nobody asks for.
    let memory = if config.llm.enabled {
        brain_task::start_memory(&store, config.embed.clone(), config.embed_api_key.clone()).await
    } else {
        None
    };
    brain.set_memory(memory.clone());
    let indexing = memory.map(|index| brain_task::spawn_indexer(index, world.clone(), store.clone(), shutdown.clone()));
    let thinking = brain_task::spawn(brain.clone(), world.clone(), store.clone(), shutdown.clone());
    let app = AppState::new(config.clone(), world, store.clone(), brain, shutdown.clone());

    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, bind = %config.bind, "cannot listen");
            store.close().await;
            return ExitCode::FAILURE;
        }
    };
    let boot = app.world.core.read().boot_id.clone();
    tracing::info!(bind = %config.bind, origin = %config.public_origin, static_dir = %config.static_dir.display(), db = %store.path().display(), boot, "listening");
    let digest = digest::task::spawn(app.clone(), shutdown.clone());
    let chat_sweeper = chat::spawn_sweeper(app.chat.clone(), shutdown.clone());

    tokio::spawn(stop_on_signal(shutdown.clone()));
    // The peer address feeds the sign-in rate limit (unless TRUST_PROXY takes it from headers).
    let service = router(app).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, service).with_graceful_shutdown(shutdown.clone().cancelled_owned()).await;
    shutdown.cancel();
    let _ = world_task.await;
    let _ = digest.await;
    let _ = chat_sweeper.await;
    let _ = thinking.await;
    if let Some(indexing) = indexing {
        let _ = indexing.await;
    }
    let _ = maintenance.await;
    // Last: flush the chronicle, checkpoint the WAL, stop the store thread.
    store.close().await;
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "server failed");
            ExitCode::FAILURE
        }
    }
}

/// SIGTERM (the platform stopping the machine) or Ctrl-C.
async fn stop_on_signal(shutdown: CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("stopping");
    shutdown.cancel();
}
