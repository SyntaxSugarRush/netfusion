// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion remote relay server.
//!
//! Optional VPS-deployed daemon that enables true multi-ISP
//! aggregation by acting as a coordinated remote endpoint.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "netfusion_relay=info".into()),
        )
        .init();

    tracing::info!("NetFusion relay server starting");

    // TODO: Initialize relay
    // - Load config
    // - Setup QUIC endpoint
    // - Accept client connections
    // - Forward traffic

    tracing::info!("NetFusion relay server exiting");

    Ok(())
}
