// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion core daemon.
//!
//! Manages network interfaces, routing, bonding, tunnels,
//! and serves the IPC endpoint for the TUI frontend.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "netfusion=info".into()),
        )
        .init();

    tracing::info!("NetFusion daemon starting");

    // TODO: Initialize core components
    // - Config loader
    // - Interface discovery
    // - Routing engine
    // - IPC server (Unix domain socket)
    // - State persistence
    // - Event bus

    tracing::info!("NetFusion daemon ready");

    // Keep alive
    tokio::signal::ctrl_c().await?;
    tracing::info!("NetFusion daemon shutting down");

    Ok(())
}
