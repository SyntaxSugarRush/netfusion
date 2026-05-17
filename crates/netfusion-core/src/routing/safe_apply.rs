// SPDX-License-Identifier: MIT OR Apache-2.0

//! Safe apply flow — validates, applies, and rolls back network changes.
//!
//! Implements the safety-critical apply flow:
//! 1. Validate configuration
//! 2. Simulate changes (dry-run)
//! 3. Apply incrementally
//! 4. Verify connectivity
//! 5. Commit changes or rollback on failure

use std::time::Duration;

use tokio::time::timeout;
use tracing::{info, warn};

use crate::error::RoutingError;
use crate::routing::engine::RoutingEngine;

/// Result of a safe apply operation.
#[derive(Debug)]
pub enum ApplyResult {
    /// Changes applied and verified successfully.
    Success,
    /// Validation failed — no changes applied.
    ValidationFailed(String),
    /// Connectivity verification failed — rolled back.
    VerificationFailed(String),
    /// Apply failed — partial rollback performed.
    ApplyFailed {
        error: String,
        rollback_success: bool,
    },
}

/// Safe apply configuration.
#[derive(Debug, Clone)]
pub struct SafeApplyConfig {
    /// Timeout for connectivity verification (seconds).
    pub verify_timeout_secs: Duration,

    /// Ping target for connectivity verification.
    pub verify_target: String,

    /// Whether to perform a dry-run before applying.
    pub dry_run: bool,
}

impl Default for SafeApplyConfig {
    fn default() -> Self {
        Self {
            verify_timeout_secs: Duration::from_secs(10),
            verify_target: "8.8.8.8".into(),
            dry_run: false,
        }
    }
}

/// Executes the safe apply flow for network changes.
pub struct SafeApply {
    config: SafeApplyConfig,
}

impl SafeApply {
    /// Create a new safe apply executor.
    pub fn new(config: SafeApplyConfig) -> Self {
        Self { config }
    }

    /// Execute the full safe apply flow.
    ///
    /// This is the critical path that ensures network changes never
    /// permanently break connectivity.
    pub async fn execute<F, Fut>(
        &self,
        routing_engine: &mut RoutingEngine,
        changes: F,
    ) -> ApplyResult
    where
        F: FnOnce(&mut RoutingEngine) -> Fut,
        Fut: std::future::Future<Output = Result<(), RoutingError>>,
    {
        // Step 1: Validate
        info!("Safe apply: validating configuration");
        if let Err(e) = self.validate().await {
            return ApplyResult::ValidationFailed(e);
        }

        // Step 2: Dry-run (if enabled)
        if self.config.dry_run {
            info!("Safe apply: dry-run mode — skipping actual changes");
            return ApplyResult::Success;
        }

        // Step 3: Save current state for rollback
        let previous_routes: Vec<_> = routing_engine.managed_routes().to_vec();

        // Step 4: Apply changes incrementally
        info!("Safe apply: applying changes");
        if let Err(e) = changes(routing_engine).await {
            warn!("Safe apply: apply failed, rolling back: {}", e);
            let rollback_ok = self.rollback(routing_engine, &previous_routes).await;
            return ApplyResult::ApplyFailed {
                error: e.to_string(),
                rollback_success: rollback_ok,
            };
        }

        // Step 5: Verify connectivity
        info!("Safe apply: verifying connectivity");
        if let Err(e) = self.verify_connectivity().await {
            warn!("Safe apply: verification failed, rolling back: {}", e);
            let rollback_ok = self.rollback(routing_engine, &previous_routes).await;
            return ApplyResult::VerificationFailed(format!(
                "connectivity check failed: {}, rollback: {}",
                e,
                if rollback_ok { "success" } else { "FAILED" }
            ));
        }

        // Step 6: Commit (just clear the rollback state)
        routing_engine.clear_managed_routes();
        info!("Safe apply: changes committed successfully");

        ApplyResult::Success
    }

    /// Validate the configuration before applying.
    async fn validate(&self) -> Result<(), String> {
        // Basic connectivity check
        if self.config.dry_run {
            return Ok(());
        }

        // Verify we can reach the internet (basic check)
        match self.ping_once(&self.config.verify_target).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("initial connectivity check failed: {}", e)),
        }
    }

    /// Verify connectivity after applying changes.
    async fn verify_connectivity(&self) -> Result<(), String> {
        timeout(
            self.config.verify_timeout_secs,
            self.ping_once(&self.config.verify_target),
        )
        .await
        .map_err(|_| "connectivity verification timed out".to_string())?
    }

    /// Rollback to the previous state.
    async fn rollback(
        &self,
        routing_engine: &mut RoutingEngine,
        previous_routes: &[crate::routing::engine::ManagedRoute],
    ) -> bool {
        info!("Rolling back {} routes", previous_routes.len());

        let mut success = true;

        // Clone the managed routes to avoid borrow conflict
        let current_routes: Vec<_> = routing_engine.managed_routes().to_vec();

        // Remove newly added routes
        for route in current_routes {
            if !previous_routes.iter().any(|r| {
                r.gateway == route.gateway
                    && r.interface == route.interface
                    && r.metric == route.metric
            }) {
                if let Err(e) = routing_engine
                    .del_default_route(&route.gateway, &route.interface, route.metric)
                    .await
                {
                    warn!("Failed to rollback route '{}': {}", route.gateway, e);
                    success = false;
                }
            }
        }

        success
    }

    /// Perform a single ping to verify connectivity.
    async fn ping_once(&self, target: &str) -> Result<(), String> {
        let output = tokio::process::Command::new("ping")
            .args(["-c", "1", "-W", "3", target])
            .output()
            .await
            .map_err(|e| format!("failed to run ping: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "ping to {} failed: {:?}",
                target,
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
