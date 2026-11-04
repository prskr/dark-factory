//! The control plane's side of the gateway links: the internal server (`PolicyWatch`,
//! `GatewayGit`, `GatewayEvents` and the stale push scan) and the `Tail` relay.

pub mod denial_events;
pub mod git_records;
mod inference_routes;
pub mod internal;
pub mod policy_service;
pub mod relay;
pub mod snapshot;
pub mod usage_records;

pub use internal::serve_internal;
pub use policy_service::PolicyService;
pub use relay::{DnsDirectory, GatewayDirectory, Relay};
pub use usage_records::TokenCounter;

#[cfg(test)]
mod tests;
