//! The Git smart-HTTP proxy for sandbox pushes (`openspec/changes/archive/2026-11-01-iter2-git-proxy`).

pub mod mirror;
pub mod protocol;
pub mod push;
pub mod quarantine;
pub mod records;
pub mod refs;
pub mod server;
pub mod spool;
pub mod upstream;
