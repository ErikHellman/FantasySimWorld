use std::process::ExitCode;

use fsw_server::config::Config;
use fsw_server::{AppState, router, world_task};
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

    let shutdown = CancellationToken::new();
    let (world, world_task) = world_task::spawn(config.initial_speed, shutdown.clone());
    let app = AppState::new(config.clone(), world, shutdown.clone());

    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, bind = %config.bind, "cannot listen");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(bind = %config.bind, origin = %config.public_origin, static_dir = %config.static_dir.display(), boot = %app.boot_id, "listening");

    tokio::spawn(stop_on_signal(shutdown.clone()));
    let served = axum::serve(listener, router(app)).with_graceful_shutdown(shutdown.clone().cancelled_owned()).await;
    shutdown.cancel();
    let _ = world_task.await;
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
