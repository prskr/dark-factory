//! The control plane's side of the gateway links: the internal server (`PolicyWatch`,
//! `GatewayGit`, `GatewayEvents` and the stale push scan) and the `Tail` relay.

pub mod denial_events;
pub mod git_records;
pub mod internal;
pub mod policy_service;
pub mod relay;
pub mod snapshot;

pub use internal::serve_internal;
pub use policy_service::PolicyService;
pub use relay::{DnsDirectory, GatewayDirectory, Relay};

#[cfg(test)]
mod tests;
