use std::net::SocketAddr;
use std::process::ExitCode;

use fsw_server::config::{AuthMode, Config};
use fsw_server::store::{self, Store};
use fsw_server::{AppState, brain_task, digest, router, world_task};
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
    if let Err(error) = store.seed_admins(&config.admin_emails).await {
        tracing::error!(%error, "cannot seed ADMIN_EMAILS");
        return ExitCode::FAILURE;
    }
    if matches!(config.auth, AuthMode::Oidc(_)) && config.admin_emails.is_empty() {
        tracing::warn!("AUTH_MODE=oidc without ADMIN_EMAILS: only people already on the allowlist can sign in");
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
    let brain = brain_task::start(world.clone(), store.clone(), config.llm_api_key.clone()).await;
    let thinking = brain_task::spawn(brain.clone(), world.clone(), store.clone(), shutdown.clone());
    let app = match AppState::new(config.clone(), world, store.clone(), brain, shutdown.clone()) {
        Ok(app) => app,
        Err(message) => {
            tracing::error!(message, "cannot set up sign-in");
            store.close().await;
            return ExitCode::FAILURE;
        }
    };

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

    tokio::spawn(stop_on_signal(shutdown.clone()));
    // The peer address feeds the sign-in rate limit (unless TRUST_PROXY takes it from headers).
    let service = router(app).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, service).with_graceful_shutdown(shutdown.clone().cancelled_owned()).await;
    shutdown.cancel();
    let _ = world_task.await;
    let _ = digest.await;
    let _ = thinking.await;
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
