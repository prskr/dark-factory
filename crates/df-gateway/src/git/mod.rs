//! The Git smart-HTTP proxy for sandbox pushes (`openspec/changes/archive/2026-11-01-iter2-git-proxy`).

pub mod checks;
pub mod mirror;
pub mod policy_checks;
pub mod protocol;
pub mod push;
pub mod quarantine;
pub mod records;
pub mod refs;
pub mod secret_rules;
pub mod server;
pub mod spool;
pub mod upstream;
pub mod view;
