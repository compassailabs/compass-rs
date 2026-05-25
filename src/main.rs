mod aave;
mod account;
mod api;
mod automation;
mod chain;
mod config;
mod contracts;
mod core;
mod gateway;
mod state;
mod userop;

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use axum::Router;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{config::AppConfig, state::AppState};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_target(false))
        .init();

    let cfg = AppConfig::from_env()?;
    let bind: SocketAddr = cfg.bind.parse()?;
    let state = AppState::new(cfg).await?;

    if state.cfg.disable_automation {
        tracing::warn!(
            "COMPASS_DISABLE_AUTOMATION=1 — snapshot worker + cron NOT spawned \
             (use for local dev against shared DB / on-chain state)"
        );
    } else {
        let snapshot_interval = Duration::from_secs(state.cfg.automation_snapshot_interval_secs);
        let _snap = automation::workers::snapshot::spawn_snapshot_worker(
            state.cfg.clone(),
            state.arbitrum_sepolia.clone(),
            state.snapshots.clone(),
            snapshot_interval,
        );
        tracing::info!(
            "automation snapshot worker spawned with interval = {}s",
            snapshot_interval.as_secs()
        );

        let cron_interval = Duration::from_secs(state.cfg.automation_cron_interval_secs);
        let _cron = automation::scheduler::spawn_cron(
            state.policies.clone(),
            state.snapshots.clone(),
            state.positions.clone(),
            state.audit.clone(),
            Some(state.position_fetcher.clone()),
            Some(state.clone()),
            cron_interval,
        );
        tracing::info!(
            "automation cron spawned with interval = {}s",
            cron_interval.as_secs()
        );
    }

    let enable_debug_api = state.cfg.enable_debug_api;
    if enable_debug_api {
        tracing::warn!(
            "ENABLE_DEBUG_API=1 — mounting /debug/* and /debug/recover-gateway/* \
             (dev only, must be off in production)"
        );
    }
    let app = Router::new()
        .merge(api::router(enable_debug_api))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    tracing::info!("compass-rs listening on http://{bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
