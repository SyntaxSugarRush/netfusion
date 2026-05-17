// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion remote relay server.
//!
//! Optional VPS-deployed daemon that enables true multi-ISP
//! aggregation by acting as a coordinated remote endpoint.
//!
//! Clients connect via QUIC and the relay forwards traffic
//! to the configured upstream destination.

mod config;
mod server;

use anyhow::Result;
use tracing::{error, info};

use crate::config::RelayConfig;
use crate::server::RelayServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "netfusion_relay=info".into()),
        )
        .init();

    info!("NetFusion relay server starting");

    // Load configuration
    let config = RelayConfig::load().unwrap_or_else(|e| {
        info!("No config file found, using defaults: {}", e);
        RelayConfig::default()
    });

    let server = RelayServer::new(&config);

    if let Err(e) = server.run().await {
        error!("Relay server error: {}", e);
        return Err(e);
    }

    info!("NetFusion relay server exiting");

    Ok(())
}
