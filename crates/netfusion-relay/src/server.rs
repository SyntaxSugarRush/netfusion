// SPDX-License-Identifier: MIT OR Apache-2.0

//! QUIC relay server implementation.

use std::sync::Arc;

use anyhow::Result;
use quinn::{Endpoint, ServerConfig};
use tracing::{info, warn};

use crate::config::RelayConfig;

/// QUIC relay server.
pub struct RelayServer {
    config: RelayConfig,
    endpoint: Option<Endpoint>,
    active_connections: usize,
}

impl RelayServer {
    pub fn new(config: &RelayConfig) -> Self {
        Self {
            config: config.clone(),
            endpoint: None,
            active_connections: 0,
        }
    }

    /// Generate self-signed certificates for development.
    fn generate_self_signed_cert(cert_path: &str, key_path: &str) -> Result<rustls::ServerConfig> {
        let cert = rcgen::generate_simple_self_signed(vec![
            "localhost".into(),
            "netfusion-relay.local".into(),
        ])?;

        let cert_der = cert.cert.der();
        let key_der = cert.signing_key.serialize_der();

        let cert_pem = cert.cert.pem();
        let key_pem = cert.signing_key.serialize_pem();

        // Save to configured paths
        std::fs::create_dir_all(
            std::path::Path::new(cert_path)
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )?;
        std::fs::write(cert_path, cert_pem)?;
        std::fs::write(key_path, key_pem)?;

        let certs = vec![rustls::pki_types::CertificateDer::from(cert_der.to_vec())];
        let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
            .map_err(|e| anyhow::anyhow!("Failed to parse key: {}", e))?;

        let mut server_config =
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        server_config.alpn_protocols = vec![b"netfusion-relay".to_vec()];

        Ok(server_config)
    }

    /// Build the QUIC server configuration.
    fn build_server_config(&self) -> Result<ServerConfig> {
        let cert_path = &self.config.cert_path;
        let key_path = &self.config.key_path;

        // Try to load existing certs, or generate self-signed
        let (cert_pem, key_pem) = if std::fs::metadata(cert_path).is_ok()
            && std::fs::metadata(key_path).is_ok()
        {
            (
                std::fs::read_to_string(cert_path)?,
                std::fs::read_to_string(key_path)?,
            )
        } else {
            info!("No TLS certificates found, generating self-signed for development");
            let _tls_config = Self::generate_self_signed_cert(cert_path, key_path)?;
            // Re-read the files we just wrote
            let cert_pem = std::fs::read_to_string(cert_path)?;
            let key_pem = std::fs::read_to_string(key_path)?;
            // We still need to build the server config from the files
            return Self::build_tls_config(&cert_pem, &key_pem);
        };

        Self::build_tls_config(&cert_pem, &key_pem)
    }

    /// Build a ServerConfig from PEM cert and key strings.
    fn build_tls_config(cert_pem: &str, key_pem: &str) -> Result<ServerConfig> {
        let cert = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
            .ok_or_else(|| anyhow::anyhow!("No private key found"))?;

        let mut tls_config =
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(
                cert.into_iter()
                    .map(rustls::pki_types::CertificateDer::from)
                    .collect(),
                rustls::pki_types::PrivateKeyDer::try_from(key.secret_der().to_vec())
                    .map_err(|e| anyhow::anyhow!("Invalid key: {}", e))?,
            )?;

        tls_config.alpn_protocols = vec![b"netfusion-relay".to_vec()];

        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)?,
        ));

        // Configure connection limits
        let mut transport_config = quinn::TransportConfig::default();
        transport_config
            .max_concurrent_bidi_streams(quinn::VarInt::from_u32(100));
        transport_config.max_concurrent_uni_streams(quinn::VarInt::from_u32(100));
        server_config.transport_config(Arc::new(transport_config));

        Ok(server_config)
    }

    /// Run the relay server.
    pub async fn run(mut self) -> Result<()> {
        let server_config = self.build_server_config()?;

        let endpoint = Endpoint::server(server_config, self.config.bind_addr.parse()?)?;
        self.endpoint = Some(endpoint);

        info!(
            bind_addr = %self.config.bind_addr,
            max_connections = self.config.max_connections,
            "Relay server listening"
        );

        // Accept connections
        if let Some(endpoint) = &self.endpoint {
            let endpoint = endpoint.clone();
            while let Some(connecting) = endpoint.accept().await {
                let remote_addr = connecting.remote_address();

                if self.active_connections >= self.config.max_connections {
                    warn!(
                        %remote_addr,
                        active = self.active_connections,
                        max = self.config.max_connections,
                        "Connection limit reached, rejecting"
                    );
                    continue;
                }

                self.active_connections += 1;

                tokio::spawn(async move {
                    match connecting.await {
                        Ok(conn) => {
                            info!(
                                %remote_addr,
                                "Client connected"
                            );
                            // Handle the connection
                            if let Err(e) = Self::handle_connection(conn).await {
                                warn!(%remote_addr, error = %e, "Connection handler error");
                            }
                        }
                        Err(e) => {
                            warn!(%remote_addr, error = %e, "Failed to accept connection");
                        }
                    }
                });
            }
        }

        Ok(())
    }

    /// Handle a single client connection.
    async fn handle_connection(conn: quinn::Connection) -> Result<()> {
        loop {
            // Accept bidirectional streams from the client
            match conn.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    // Echo back for now — actual forwarding would go here
                    match recv.read_to_end(1024 * 1024).await {
                        Ok(buf) => {
                            if let Err(e) = send.write_all(&buf).await {
                                warn!(error = %e, "Failed to send response");
                            }
                            let _ = send.finish();
                        }
                        Err(e) => {
                            warn!(error = %e, "Stream read error");
                            break;
                        }
                    }
                }
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    info!("Client disconnected");
                    break;
                }
                Err(e) => {
                    warn!(error = %e, "Stream accept error");
                    break;
                }
            }
        }

        Ok(())
    }
}
