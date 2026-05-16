// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion terminal user interface.
//!
//! A modern, keyboard-driven TUI for monitoring and managing
//! network aggregation, bonding, and routing.

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

    tracing::info!("NetFusion TUI starting");

    // TODO: Initialize TUI
    // - Connect to daemon via IPC
    // - Setup crossterm + ratatui terminal
    // - Start event loop
    // - Render dashboard

    tracing::info!("NetFusion TUI exiting");

    Ok(())
}
