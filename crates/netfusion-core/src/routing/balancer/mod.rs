// SPDX-License-Identifier: MIT OR Apache-2.0

//! ECMP and weighted balancing engine.
//!
//! Implements:
//! - ECMP routing (multiple equal-cost default routes)
//! - Weighted balancing based on health scores
//! - nftables mark-based flow distribution

mod ecmp;
mod weighted;
mod nftables;

pub use ecmp::EcmpRouter;
pub use weighted::WeightedBalancer;
