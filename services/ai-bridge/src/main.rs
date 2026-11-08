mod admin;
mod auth;
mod auth_failure;
mod config;
mod forensic_writer;
mod logger;
mod observability;
mod private_file;
mod proxy;
mod rate_limit;
mod responses_chat;
mod sse_image;
mod structured_outputs;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{Router, routing::any};
use tracing::{info, warn};

use crate::{config::Config, proxy::AppState};

const PROTOCOL_ROUTE_ROOTS: &[&str] = &[
    "/v1",
    "/v1beta",
    "/mj",
    "/suno",
    "/kling/v1",
    "/jimeng",
    "/pg",
];
const PROTOCOL_ROUTE_PATTERNS: &[&str] = &["/:mode/mj"];

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let observability = Arc::new(observability::Observability::new());
    observability::install_global(observability.clone());
    let log_guard = logger::init_tracing(&config)?;
    log_guard.keepalive_marker();

    let listen_addr = config.listen_addr;
    let upstream_base_url = config.upstream_base_url.clone();
    let reload_interval = config.config_reload_interval;
    let auto_restart_interval = config.auto_restart_interval;
    let app_state = Arc::new(AppState::new_with_observability(config, observability)?);
    if let Some(interval) = reload_interval {
        spawn_config_reload_task(app_state.clone(), interval);
    }

    let app = admin::mount_routes(mount_protocol_routes(Router::new())).with_state(app_state);

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    info!(
        listen_addr = %listen_addr,
        upstream_base_url = %upstream_base_url,
        auto_restart_interval_secs = auto_restart_interval.map(|interval| interval.as_secs()),
        "ai-bridge proxy started"
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(auto_restart_interval))
    .await?;

    Ok(())
}

fn spawn_config_reload_task(app_state: Arc<AppState>, interval: Duration) {
    tokio::spawn(async move {
        let mut interval = interval;

        loop {
            tokio::time::sleep(interval).await;

            let app_state_clone = app_state.clone();
            match tokio::task::spawn_blocking(move || reload_runtime_config(&app_state_clone)).await
            {
                Ok(Ok(Some(next_interval))) => {
                    app_state.observability.record_reload(true, "automatic");
                    interval = next_interval;
                }
                Ok(Ok(None)) => {
                    app_state.observability.record_reload(true, "automatic");
                    info!(
                        event = "config_reload_disabled",
                        "runtime configuration reload disabled by current configuration"
                    );
                    break;
                }
                Ok(Err(error)) => {
                    app_state.observability.record_reload(false, "automatic");
                    warn!(
                        event = "config_reload_failed",
                        error = %error,
                        "failed to reload runtime configuration; keeping previous config"
                    );
                }
                Err(join_error) => {
                    app_state.observability.record_reload(false, "automatic");
                    warn!(
                        event = "config_reload_task_panicked",
                        error = %join_error,
                        "config reload blocking task panicked; keeping previous config"
                    );
                }
            }
        }
    });
}

fn reload_runtime_config(app_state: &AppState) -> Result<Option<Duration>> {
    let config = Config::from_env()?;
    let upstream_base_url = config.upstream_base_url.clone();
    let reload_interval = config.config_reload_interval;
    let reload_interval_secs = reload_interval.map(|interval| interval.as_secs());
    let stream_event_stall_retry_enabled = config.stream_event_stall_retry_enabled;
    let stream_event_stall_timeout_secs = config
        .stream_event_stall_timeout
        .map(|timeout| timeout.as_secs());
    let upstream_502_cooldown_enabled = config.upstream_502_cooldown_enabled;
    let upstream_error_cooldown_enabled = config.upstream_error_cooldown_enabled;
    let cyber_policy_confirmation_rules_file = config
        .cyber_policy_confirmation_rules_file
        .as_ref()
        .map(|path| path.display().to_string());

    app_state.reload_runtime(config)?;

    info!(
        event = "config_reloaded",
        upstream_base_url = %upstream_base_url,
        reload_interval_secs,
        stream_event_stall_retry_enabled,
        stream_event_stall_timeout_secs,
        upstream_502_cooldown_enabled,
        upstream_error_cooldown_enabled,
        cyber_policy_confirmation_rules_file,
        "runtime configuration reloaded"
    );

    Ok(reload_interval)
}

fn mount_protocol_routes(mut router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    for root in PROTOCOL_ROUTE_ROOTS {
        router = mount_protocol_route(router, root);
    }
    for pattern in PROTOCOL_ROUTE_PATTERNS {
        router = mount_protocol_route(router, pattern);
    }
    router
}

fn mount_protocol_route(router: Router<Arc<AppState>>, route: &str) -> Router<Arc<AppState>> {
    let wildcard = format!("{route}/*path");
    router
        .route(route, any(proxy::handle_protocol))
        .route(&wildcard, any(proxy::handle_protocol))
}

async fn shutdown_signal(auto_restart_interval: Option<Duration>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    let auto_restart = async move {
        match auto_restart_interval {
            Some(interval) => tokio::time::sleep(interval).await,
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = ctrl_c => {
            info!(event = "shutdown_signal", signal = "ctrl_c", "shutdown signal received");
        },
        _ = terminate => {
            info!(event = "shutdown_signal", signal = "sigterm", "shutdown signal received");
        },
        _ = auto_restart => {
            info!(
                event = "auto_restart_due",
                auto_restart_interval_secs = auto_restart_interval.map(|interval| interval.as_secs()),
                "auto-restart interval reached; shutting down for supervisor restart"
            );
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_route_roots_cover_model_protocols_without_admin_api() {
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/v1"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/v1beta"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/mj"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/suno"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/kling/v1"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/jimeng"));
        assert!(PROTOCOL_ROUTE_ROOTS.contains(&"/pg"));
        assert!(PROTOCOL_ROUTE_PATTERNS.contains(&"/:mode/mj"));
        assert!(!PROTOCOL_ROUTE_ROOTS.contains(&"/api"));
        assert!(!PROTOCOL_ROUTE_PATTERNS.contains(&"/api"));
    }

    #[test]
    fn protocol_routes_can_be_mounted() {
        let _router = mount_protocol_routes(Router::<Arc<AppState>>::new());
    }

    #[test]
    fn admin_routes_can_be_mounted_with_protocol_routes() {
        let _router = admin::mount_routes(mount_protocol_routes(Router::<Arc<AppState>>::new()));
    }
}
