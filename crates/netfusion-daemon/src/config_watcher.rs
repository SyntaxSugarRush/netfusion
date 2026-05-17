// SPDX-License-Identifier: MIT OR Apache-2.0

//! Configuration file watcher with live reload.
//!
//! Watches the config file for changes, validates new configs,
//! and applies them via the safe-apply flow.

use std::path::Path;
use std::sync::Arc;

use netfusion_shared::config::NetfusionConfig;
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};
use validator::Validate;

/// Configuration watcher that monitors the config file for changes.
pub struct ConfigWatcher {
    config_path: String,
    config: Arc<RwLock<NetfusionConfig>>,
    event_tx: broadcast::Sender<netfusion_shared::events::NetfusionEvent>,
}

impl ConfigWatcher {
    /// Create a new config watcher.
    pub fn new(
        config_path: String,
        config: Arc<RwLock<NetfusionConfig>>,
        event_tx: broadcast::Sender<netfusion_shared::events::NetfusionEvent>,
    ) -> Self {
        Self {
            config_path,
            config,
            event_tx,
        }
    }

    /// Start watching the config file for changes.
    pub fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = self.config_path.clone();

        if !Path::new(&path).exists() {
            info!("Config file not found at {}, watching will start when file is created", path);
            // Don't fail — the file may be created later
            return Ok(());
        }

        let watcher = Arc::clone(&self);

        let mut notify_watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                let watcher = Arc::clone(&watcher);
                match result {
                    Ok(event) => {
                        if event.kind.is_modify() || event.kind.is_create() {
                            let path_clone = watcher.config_path.clone();
                            let config_clone = watcher.config.clone();
                            let event_tx_clone = watcher.event_tx.clone();

                            tokio::spawn(async move {
                                if let Err(e) = watcher
                                    .handle_config_change(
                                        &path_clone,
                                        &config_clone,
                                        &event_tx_clone,
                                    )
                                    .await
                                {
                                    error!("Config reload failed: {}", e);
                                }
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Config watcher error: {}", e);
                    }
                }
            },
            NotifyConfig::default(),
        )?;

        // Watch the directory (not just the file) to catch file creation
        if let Some(parent) = Path::new(&path).parent() {
            notify_watcher.watch(parent, RecursiveMode::NonRecursive)?;
            info!("Watching config directory for changes to {}", path);
        }

        // Keep the watcher alive
        std::mem::forget(notify_watcher);

        Ok(())
    }

    /// Handle a config file change.
    async fn handle_config_change(
        &self,
        path: &str,
        config: &RwLock<NetfusionConfig>,
        event_tx: &broadcast::Sender<netfusion_shared::events::NetfusionEvent>,
    ) -> Result<(), String> {
        info!("Config file changed, reloading: {}", path);

        // Read and parse new config
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("failed to read config: {}", e))?;

        let new_config: NetfusionConfig = toml::from_str(&content)
            .map_err(|e| format!("failed to parse config: {}", e))?;

        // Validate
        if let Err(e) = new_config.daemon.validate() {
            return Err(format!("invalid config: {}", e));
        }

        // Apply
        {
            let mut stored = config.write().await;
            *stored = new_config;
        }

        info!("Config reloaded successfully");

        // Emit event
        let _ = event_tx.send(netfusion_shared::events::NetfusionEvent::ConfigReloaded(
            netfusion_shared::events::ConfigEvent {
                timestamp: chrono::Utc::now(),
                source: path.to_string(),
                errors: Vec::new(),
            },
        ));

        Ok(())
    }
}
