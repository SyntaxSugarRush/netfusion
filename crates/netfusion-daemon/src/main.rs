// SPDX-License-Identifier: MIT OR Apache-2.0

//! NetFusion core daemon.
//!
//! Manages network interfaces, routing, bonding, tunnels,
//! and serves the IPC endpoint for the TUI frontend.

mod ipc;

use std::sync::Arc;

use anyhow::Result;
use netfusion_shared::config::NetfusionConfig;
use netfusion_shared::events::NetfusionEvent;
use netfusion_shared::types::InterfaceInfo;
use netfusion_core::discovery::InterfaceScanner;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::ipc::IpcServer;

const DEFAULT_SOCKET_PATH: &str = "/run/netfusion/netfusion.sock";
const SCAN_INTERVAL_SECS: u64 = 30;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "netfusion=info".into()),
        )
        .init();

    info!("NetFusion daemon starting");

    // Load configuration
    let config = load_config().await?;

    // Shared state
    let interfaces: Arc<RwLock<Vec<InterfaceInfo>>> = Arc::new(RwLock::new(Vec::new()));
    let config_store: Arc<RwLock<NetfusionConfig>> = Arc::new(RwLock::new(config));
    let (event_tx, _) = broadcast::channel::<NetfusionEvent>(1024);

    // Start interface scanner
    let scanner = match InterfaceScanner::new() {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to create interface scanner: {}", e);
            return Ok(());
        }
    };

    // Initial scan
    {
        let mut ifaces = interfaces.write().await;
        match scanner.scan().await {
            Ok(found) => {
                info!("Initial scan found {} interfaces", found.len());
                *ifaces = found;
            }
            Err(e) => {
                error!("Initial scan failed: {}", e);
            }
        }
    }

    // Periodic rescan
    {
        let interfaces = interfaces.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(SCAN_INTERVAL_SECS));
            loop {
                interval.tick().await;
                match scanner.scan().await {
                    Ok(found) => {
                        let mut ifaces = interfaces.write().await;
                        *ifaces = found;
                        info!("Rescan found {} interfaces", ifaces.len());
                    }
                    Err(e) => {
                        error!("Rescan failed: {}", e);
                    }
                }
            }
        });
    }

    // Start IPC server
    let socket_path = config_store
        .read()
        .await
        .daemon
        .socket_path
        .clone();

    let ipc_server = Arc::new(IpcServer::new(
        socket_path,
        interfaces.clone(),
        config_store.clone(),
        event_tx.clone(),
    ));

    tokio::spawn(async move {
        if let Err(e) = ipc_server.run().await {
            error!("IPC server error: {}", e);
        }
    });

    info!("NetFusion daemon ready");

    // Keep alive
    tokio::signal::ctrl_c().await?;
    info!("NetFusion daemon shutting down");

    // Cleanup socket
    let _ = std::fs::remove_file(DEFAULT_SOCKET_PATH);

    Ok(())
}

/// Load configuration from file or return defaults.
async fn load_config() -> Result<NetfusionConfig> {
    let config_path = "/etc/netfusion/netfusion.toml";

    if std::path::Path::new(config_path).exists() {
        match tokio::fs::read_to_string(config_path).await {
            Ok(content) => {
                let config: NetfusionConfig =
                    toml::from_str(&content).map_err(|e| {
                        anyhow::anyhow!("Failed to parse config: {}", e)
                    })?;
                info!("Loaded configuration from {}", config_path);
                return Ok(config);
            }
            Err(e) => {
                warn!("Failed to read config file: {}", e);
            }
        }
    }

    info!("Using default configuration");
    Ok(NetfusionConfig::default())
}
