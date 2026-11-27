//! The egress gateway: allowlisting HTTP(S) proxy for sandboxes (`design/egress.md`).
//!
//! `dark-factory gateway` runs [`proxy`], [`tail`] and [`watch`] (no database access). The
//! control plane's side of the links lives in `df-gateway-control`.

pub mod convert;
mod count;
pub mod denials;
pub mod flow;
pub mod gateway;
pub mod git;
pub mod health;
pub mod inference;
pub mod link;
pub mod mcp;
pub mod metrics;
pub mod peer;
pub mod policy;
pub mod proxy;
pub mod resolve;
mod sni;
pub mod tail;
pub mod upstream;
pub mod watch;
