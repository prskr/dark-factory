//! The control plane's side of the gateway links: the `PolicyWatch` server and the `Tail` relay.

pub mod policy_service;
pub mod relay;
pub mod snapshot;

pub use policy_service::{PolicyService, serve as serve_policy};
pub use relay::{DnsDirectory, GatewayDirectory, Relay};

#[cfg(test)]
mod tests;
