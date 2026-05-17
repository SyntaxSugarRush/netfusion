// SPDX-License-Identifier: MIT OR Apache-2.0

//! IPC client — connects to the daemon from the TUI.

use std::path::Path;

use netfusion_shared::ipc::{
    DaemonRequest, DaemonResponse, ResponseData, WireRequest,
    WireResponse,
};
use netfusion_shared::types::InterfaceInfo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use thiserror::Error;

/// IPC client error.
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("connection failed: {0}")]
    Connect(#[source] std::io::Error),

    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),

    #[error("encoding error: {0}")]
    Encode(#[source] bincode::Error),

    #[error("server error: {0}")]
    Server(String),
}

/// Client for communicating with the NetFusion daemon.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// Connect to the daemon via Unix domain socket.
    pub async fn connect(socket_path: &str) -> Result<Self, IpcError> {
        if !Path::new(socket_path).exists() {
            return Err(IpcError::Connect(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("daemon socket not found at {}", socket_path),
            )));
        }

        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(IpcError::Connect)?;

        Ok(Self { stream })
    }

    /// Send a request and receive the response.
    pub async fn request(&mut self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        let wire_req = WireRequest::new(req);
        let bytes = bincode::serialize(&wire_req).map_err(IpcError::Encode)?;

        // Send frame size + data
        let len = bytes.len() as u32;
        self.stream
            .write_all(&len.to_be_bytes())
            .await
            .map_err(IpcError::Io)?;
        self.stream.write_all(&bytes).await.map_err(IpcError::Io)?;

        // Read response frame
        let mut frame_size = [0u8; 4];
        self.stream
            .read_exact(&mut frame_size)
            .await
            .map_err(IpcError::Io)?;
        let frame_size = u32::from_be_bytes(frame_size) as usize;

        let mut buf = vec![0u8; frame_size];
        self.stream.read_exact(&mut buf).await.map_err(IpcError::Io)?;

        let wire_resp: WireResponse =
            bincode::deserialize(&buf).map_err(IpcError::Encode)?;

        match wire_resp.payload {
            DaemonResponse::Ok { data } => Ok(DaemonResponse::Ok { data }),
            DaemonResponse::Error { message, recoverable } => {
                Ok(DaemonResponse::Error { message, recoverable })
            }
        }
    }

    /// Get the system status.
    pub async fn get_status(&mut self) -> Result<netfusion_shared::types::SystemStatus, IpcError> {
        match self.request(DaemonRequest::GetStatus).await? {
            DaemonResponse::Ok {
                data: Some(ResponseData::Status(s)),
            } => Ok(s),
            DaemonResponse::Ok { .. } => Err(IpcError::Server(
                "unexpected response type for GetStatus".into(),
            )),
            DaemonResponse::Error { message, .. } => Err(IpcError::Server(message)),
        }
    }

    /// Get all interfaces.
    pub async fn get_interfaces(&mut self) -> Result<Vec<InterfaceInfo>, IpcError> {
        match self.request(DaemonRequest::GetInterfaces).await? {
            DaemonResponse::Ok {
                data: Some(ResponseData::Interfaces(interfaces)),
            } => Ok(interfaces),
            DaemonResponse::Ok { .. } => Err(IpcError::Server(
                "unexpected response type for GetInterfaces".into(),
            )),
            DaemonResponse::Error { message, .. } => Err(IpcError::Server(message)),
        }
    }
}
