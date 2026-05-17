// SPDX-License-Identifier: MIT OR Apache-2.0

//! IPC server module for the daemon.

pub mod server;
pub mod client;

pub use server::IpcServer;
pub use client::IpcClient;
