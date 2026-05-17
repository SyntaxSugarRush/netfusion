// SPDX-License-Identifier: MIT OR Apache-2.0

//! IPC server — listens on a Unix domain socket and handles daemon requests.

use std::path::Path;
use std::sync::Arc;

use netfusion_shared::config::NetfusionConfig;
use netfusion_shared::ipc::{
    DaemonRequest, DaemonResponse, ResponseData, WireRequest,
    WireResponse, IPC_PROTOCOL_VERSION,
};
use netfusion_shared::types::{InterfaceInfo, SystemStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};
use validator::Validate;

/// The IPC server that handles TUI connections.
pub struct IpcServer {
    socket_path: String,
    interfaces: Arc<RwLock<Vec<InterfaceInfo>>>,
    config: Arc<RwLock<NetfusionConfig>>,
    event_tx: broadcast::Sender<netfusion_shared::events::NetfusionEvent>,
    uptime_start: std::time::Instant,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(
        socket_path: String,
        interfaces: Arc<RwLock<Vec<InterfaceInfo>>>,
        config: Arc<RwLock<NetfusionConfig>>,
        event_tx: broadcast::Sender<netfusion_shared::events::NetfusionEvent>,
    ) -> Self {
        Self {
            socket_path,
            interfaces,
            config,
            event_tx,
            uptime_start: std::time::Instant::now(),
        }
    }

    /// Start the IPC server and listen for connections.
    pub async fn run(self: Arc<Self>) -> std::io::Result<()> {
        let path = Path::new(&self.socket_path);

        // Remove existing socket
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        // Create parent directory
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let listener = UnixListener::bind(path)?;
        info!("IPC server listening on {}", self.socket_path);

        loop {
            let (stream, _) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(stream).await {
                    error!("IPC connection error: {}", e);
                }
            });
        }
    }

    /// Handle a single client connection.
    async fn handle_connection(&self, mut stream: UnixStream) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(4096);

        loop {
            let mut frame_size_buf = [0u8; 4];
            match stream.read_exact(&mut frame_size_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(()); // Client disconnected
                }
                Err(e) => return Err(e),
            }

            let frame_size = u32::from_be_bytes(frame_size_buf) as usize;
            if frame_size > 10 * 1024 * 1024 {
                // 10MB max
                warn!("IPC frame too large: {} bytes", frame_size);
                return Ok(());
            }

            buf.resize(frame_size, 0);
            stream.read_exact(&mut buf).await?;

            // Decode the request
            let request: WireRequest = match bincode::deserialize(&buf) {
                Ok(req) => req,
                Err(e) => {
                    warn!("Failed to decode IPC request: {}", e);
                    continue;
                }
            };

            // Validate protocol version
            if request.version != IPC_PROTOCOL_VERSION {
                warn!(
                    "Protocol version mismatch: client={}, server={}",
                    request.version, IPC_PROTOCOL_VERSION
                );
            }

            // Process the request
            let response = self.process_request(request.payload).await;
            let wire_response = WireResponse::new(response);

            // Encode and send
            let response_bytes =
                bincode::serialize(&wire_response).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                })?;

            let response_len = response_bytes.len() as u32;
            stream.write_all(&response_len.to_be_bytes()).await?;
            stream.write_all(&response_bytes).await?;
        }
    }

    /// Process a daemon request and return a response.
    async fn process_request(&self, request: DaemonRequest) -> DaemonResponse {
        match request {
            DaemonRequest::GetStatus => {
                let interfaces = self.interfaces.read().await;
                let status = SystemStatus {
                    total_interfaces: interfaces.len(),
                    active_bonds: 0, // TODO: get from bond manager
                    connected_tunnels: 0, // TODO: get from tunnel manager
                    active_profile: None, // TODO: track active profile
                    health: None, // TODO: compute aggregate health
                    failover_active: false,
                    dry_run: false,
                    uptime_secs: self.uptime_start.elapsed().as_secs(),
                    timestamp: chrono::Utc::now(),
                };
                self.success(Some(ResponseData::Status(status)))
            }
            DaemonRequest::GetInterfaces => {
                let interfaces = self.interfaces.read().await;
                self.success(Some(ResponseData::Interfaces(interfaces.clone())))
            }
            DaemonRequest::GetInterface { name } => {
                let interfaces = self.interfaces.read().await;
                if let Some(iface) = interfaces.iter().find(|i| i.name == name) {
                    self.success(Some(ResponseData::Interface(iface.clone())))
                } else {
                    self.error(format!("Interface '{}' not found", name), false)
                }
            }
            DaemonRequest::GetConfig => {
                let config = self.config.read().await;
                self.success(Some(ResponseData::Config(config.clone())))
            }
            DaemonRequest::ApplyConfig { config } => {
                // Validate config
                if let Err(e) = config.daemon.validate() {
                    return self.error(format!("Invalid config: {}", e), true);
                }

                let mut stored = self.config.write().await;
                *stored = config;

                info!("Configuration applied");
                self.success(Some(ResponseData::Empty))
            }
            DaemonRequest::DryRunConfig { config } => {
                match config.daemon.validate() {
                    Ok(_) => self.success(Some(ResponseData::Empty)),
                    Err(e) => self.error(format!("Invalid config: {}", e), true),
                }
            }
            DaemonRequest::RescanInterfaces => {
                // TODO: trigger rescan via discovery
                self.success(Some(ResponseData::Empty))
            }
            DaemonRequest::GetEvents { limit: _ } => {
                // TODO: store events and return recent ones
                self.success(Some(ResponseData::Events(Vec::new())))
            }
            DaemonRequest::GetBonds => {
                // TODO: return bond states
                self.success(Some(ResponseData::Bonds(Vec::new())))
            }
            DaemonRequest::GetBond { name: _ } => {
                // TODO: return specific bond state
                self.success(None)
            }
            DaemonRequest::GetTunnels => {
                // TODO: return tunnel states
                self.success(Some(ResponseData::Tunnels(Vec::new())))
            }
            DaemonRequest::SubscribeEvents => {
                // Subscriptions are handled via a separate mechanism
                self.success(Some(ResponseData::Empty))
            }
            DaemonRequest::UnsubscribeEvents => {
                self.success(Some(ResponseData::Empty))
            }
            DaemonRequest::ActivateProfile { name } => {
                // TODO: activate profile
                info!("Activating profile: {}", name);
                self.success(Some(ResponseData::Profile(Some(name))))
            }
            DaemonRequest::DeactivateProfile => {
                // TODO: deactivate profile
                self.success(Some(ResponseData::Profile(None)))
            }
            DaemonRequest::GetActiveProfile => {
                // TODO: return active profile
                self.success(Some(ResponseData::Profile(None)))
            }
            DaemonRequest::GetHealth { interface } => {
                let interfaces = self.interfaces.read().await;
                if let Some(iface) = interfaces.iter().find(|i| i.name == interface) {
                    if let Some(ref health) = iface.health {
                        self.success(Some(ResponseData::Health(health.clone())))
                    } else {
                        self.error("No health data available".into(), false)
                    }
                } else {
                    self.error(format!("Interface '{}' not found", interface), false)
                }
            }
            DaemonRequest::GetAllHealth => {
                let interfaces = self.interfaces.read().await;
                let health: Vec<_> = interfaces
                    .iter()
                    .filter_map(|i| i.health.clone().map(|h| (i.name.clone(), h)))
                    .collect();
                self.success(Some(ResponseData::AllHealth(health)))
            }
            DaemonRequest::CreateBond { config: _ } => {
                // TODO: create bond via bond manager
                self.error("Bond creation not yet implemented".into(), true)
            }
            DaemonRequest::DeleteBond { name: _ } => {
                // TODO: delete bond via bond manager
                self.error("Bond deletion not yet implemented".into(), true)
            }
            DaemonRequest::EmergencyRollback => {
                warn!("Emergency rollback requested");
                // TODO: rollback all changes
                self.success(Some(ResponseData::Empty))
            }
            DaemonRequest::Shutdown => {
                info!("Shutdown requested via IPC");
                self.success(Some(ResponseData::Empty))
            }
        }
    }

    fn success(&self, data: Option<ResponseData>) -> DaemonResponse {
        DaemonResponse::Ok { data }
    }

    fn error(&self, message: String, recoverable: bool) -> DaemonResponse {
        DaemonResponse::Error { message, recoverable }
    }
}
