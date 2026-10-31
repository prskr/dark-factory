//! The egress gateway: allowlisting HTTP(S) proxy for sandboxes (`design/egress.md`), and the
//! control plane's side of its policy and log links.
//!
//! `dark-factory gateway` runs [`proxy`], [`tail`] and [`watch`] (no database access). The
//! control plane serves [`control::PolicyService`] and relays logs with [`control::Relay`].

pub mod control;
mod count;
pub mod flow;
pub mod gateway;
pub mod git;
pub mod health;
pub mod link;
pub mod metrics;
pub mod peer;
pub mod policy;
pub mod proxy;
pub mod resolve;
mod sni;
pub mod tail;
pub mod watch;
